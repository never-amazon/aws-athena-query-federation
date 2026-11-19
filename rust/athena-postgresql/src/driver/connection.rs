use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use athena_jdbc::connection::{JdbcDriver, JdbcProperties};
use athena_jdbc::sql::{
    ColumnMetadata, DatabaseMetaData, JdbcConnection, Nullability, PreparedStatement, ResultSet,
    ResultSetMetaData, RowSource, SqlError, SqlValue,
};
use tokio::sync::Mutex;

use super::error::{
    psql, psql_with_cause, CONNECTION_FAILURE, DATA_TYPE_MISMATCH, INVALID_PARAMETER_VALUE, NO_DATA,
};
use super::metadata::PgDatabaseMetaData;
use super::parser::{to_native_sql, NativeSql};
use super::session::{lock_buffer, FieldDescription, Session, SharedBuffer};
use super::startup::{connect, send_cancel, CancelTarget};
use super::types::TypeCache;
use super::url::parse_url;

pub const POSTGRESQL_DRIVER_CLASS: &str = "org.postgresql.Driver";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PgDriver;

#[async_trait]
impl JdbcDriver for PgDriver {
    fn driver_class_name(&self) -> &str {
        POSTGRESQL_DRIVER_CLASS
    }

    async fn connect(
        &self,
        url: &str,
        properties: &JdbcProperties,
    ) -> Result<Box<dyn JdbcConnection>, SqlError> {
        Ok(Box::new(PgConnection::connect(url, properties).await?))
    }
}

pub(crate) type SharedSession = Arc<Mutex<Session>>;

pub struct PgConnection {
    session: SharedSession,
    metadata: PgDatabaseMetaData,
    cancel: Option<CancelTarget>,
}

struct QueryTimer(Option<tokio::task::JoinHandle<()>>);

impl Drop for QueryTimer {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

#[derive(Debug, Clone, Default)]
struct FieldMetadata {
    table: String,
    nullable: bool,
    auto_increment: bool,
}

const FIELD_METADATA_QUERY_PREFIX: &str = "SELECT c.oid, a.attnum, a.attname, c.relname, n.nspname, a.attnotnull OR (t.typtype = 'd' AND t.typnotnull), a.attidentity != '' OR pg_catalog.pg_get_expr(d.adbin, d.adrelid) LIKE '%nextval(%' FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON (c.relnamespace = n.oid) JOIN pg_catalog.pg_attribute a ON (c.oid = a.attrelid) JOIN pg_catalog.pg_type t ON (a.atttypid = t.oid) LEFT JOIN pg_catalog.pg_attrdef d ON (d.adrelid = a.attrelid AND d.adnum = a.attnum) JOIN (";

fn column_metadata(
    types: &TypeCache,
    field: &FieldDescription,
    extra: Option<&FieldMetadata>,
) -> ColumnMetadata {
    let type_oid = field.type_oid;
    let typmod = field.type_modifier;
    let mut type_name = types.pg_type_name(type_oid);
    if extra.is_some_and(|m| m.auto_increment) {
        type_name = match type_name.as_str() {
            "int4" => "serial".to_string(),
            "int8" => "bigserial".to_string(),
            "int2" => "smallserial".to_string(),
            _ => type_name,
        };
    }
    let nullability = match extra {
        Some(m) if !m.nullable => Nullability::NoNulls,
        Some(_) => Nullability::Nullable,
        None => Nullability::Unknown,
    };
    ColumnMetadata::new(field.name.clone(), types.sql_type(type_oid))
        .with_type_name(type_name)
        .with_precision(types.precision(type_oid, typmod))
        .with_scale(types.scale(type_oid, typmod))
        .with_display_size(types.display_size(type_oid, typmod))
        .with_nullability(nullability)
        .with_table_name(extra.map(|m| m.table.clone()).unwrap_or_default())
}

pub(crate) fn result_metadata(types: &TypeCache, fields: &[FieldDescription]) -> ResultSetMetaData {
    ResultSetMetaData::new(
        fields
            .iter()
            .map(|f| column_metadata(types, f, None))
            .collect(),
    )
}

struct PgRowSource {
    session: SharedSession,
    id: u64,
    buffer: SharedBuffer,
    timer: QueryTimer,
}

#[async_trait]
impl RowSource for PgRowSource {
    async fn next_row(&mut self) -> Result<Option<Vec<SqlValue>>, SqlError> {
        let next = self.read_row().await;
        if !matches!(next, Ok(Some(_))) {
            self.timer = QueryTimer(None);
        }
        next
    }
}

impl PgRowSource {
    async fn read_row(&mut self) -> Result<Option<Vec<SqlValue>>, SqlError> {
        if let Some(next) = lock_buffer(&self.buffer).pop() {
            return next;
        }
        let mut session = self.session.lock().await;
        if session.active_id() == Some(self.id) {
            return session.next_row(self.id, true).await;
        }
        if session.has_portal(self.id) {
            session.resume(self.id).await?;
            return session.next_row(self.id, true).await;
        }
        drop(session);
        let mut buffer = lock_buffer(&self.buffer);
        if let Some(next) = buffer.pop() {
            return next;
        }
        buffer.abandon();
        Ok(None)
    }
}

impl Drop for PgRowSource {
    fn drop(&mut self) {
        lock_buffer(&self.buffer).abandon();
    }
}

fn native_sql(statement: &PreparedStatement) -> NativeSql {
    to_native_sql(statement.sql())
}

fn check_parameters(native: &NativeSql, statement: &PreparedStatement) -> Result<(), SqlError> {
    let bound = statement.parameters().len();
    if bound > native.parameter_count {
        return Err(psql(
            format!(
                "The column index is out of range: {bound}, number of columns: {}.",
                native.parameter_count
            ),
            INVALID_PARAMETER_VALUE,
        ));
    }
    if bound < native.parameter_count {
        return Err(psql(
            format!("No value specified for parameter {}.", bound + 1),
            INVALID_PARAMETER_VALUE,
        ));
    }
    Ok(())
}

impl PgConnection {
    pub async fn connect(url: &str, properties: &JdbcProperties) -> Result<PgConnection, SqlError> {
        let info: HashMap<String, String> = properties.clone();
        let settings = parse_url(url, &info)?;
        let (session, host) = connect(&settings).await?;
        let cancel = session.backend_key().map(|key| CancelTarget {
            host,
            key,
            timeout: settings.cancel_signal_timeout,
        });
        let session: SharedSession = Arc::new(Mutex::new(session));
        Ok(PgConnection {
            metadata: PgDatabaseMetaData::new(Arc::clone(&session)),
            session,
            cancel,
        })
    }

    pub async fn cancel_query(&self) -> Result<(), SqlError> {
        let Some(target) = &self.cancel else {
            return Ok(());
        };
        send_cancel(target).await.map_err(|e| {
            psql_with_cause(
                "Unable to send the cancel request to the server.",
                CONNECTION_FAILURE,
                e,
            )
        })
    }

    fn query_timer(&self, statement: &PreparedStatement) -> QueryTimer {
        let (Some(limit), Some(target)) = (statement.query_timeout(), self.cancel.clone()) else {
            return QueryTimer(None);
        };
        QueryTimer(Some(tokio::spawn(async move {
            tokio::time::sleep(limit).await;
            if let Err(e) = send_cancel(&target).await {
                tracing::debug!("Failed to cancel the statement after its timeout: {}", e);
            }
        })))
    }

    async fn field_metadata(
        &self,
        session: &mut Session,
        fields: &[FieldDescription],
    ) -> Result<Vec<Option<FieldMetadata>>, SqlError> {
        let keys: Vec<(u32, i16)> = fields
            .iter()
            .filter(|f| f.table_oid != 0)
            .map(|f| (f.table_oid, f.column))
            .collect();
        if keys.is_empty() {
            return Ok(vec![None; fields.len()]);
        }
        let mut seen = BTreeSet::new();
        let mut values = Vec::new();
        for (i, (table, column)) in keys.iter().enumerate() {
            if !seen.insert((*table, *column)) {
                continue;
            }
            if i == 0 {
                values.push(format!("SELECT {table} AS oid , {column} AS attnum"));
            } else {
                values.push(format!("SELECT {table}, {column}"));
            }
        }
        let tables: BTreeSet<u32> = keys.iter().map(|(t, _)| *t).collect();
        let sql = format!(
            "{FIELD_METADATA_QUERY_PREFIX}{}) vals ON (c.oid = vals.oid AND a.attnum = vals.attnum) where c.oid in ({})",
            values.join(" UNION ALL "),
            tables
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        let (_, rows) = session.query_all(&sql, &[], true).await?;
        let mut found: HashMap<(u32, i16), FieldMetadata> = HashMap::new();
        for row in rows {
            let text = |i: usize| {
                row.get(i)
                    .and_then(SqlValue::to_java_string)
                    .unwrap_or_default()
            };
            let table_oid = text(0).parse::<u32>().unwrap_or(0);
            let column = text(1).parse::<i16>().unwrap_or(0);
            found.insert(
                (table_oid, column),
                FieldMetadata {
                    table: text(3),
                    nullable: text(5) != "t",
                    auto_increment: text(6) == "t",
                },
            );
        }
        Ok(fields
            .iter()
            .map(|f| found.get(&(f.table_oid, f.column)).cloned())
            .collect())
    }
}

#[async_trait]
impl JdbcConnection for PgConnection {
    fn metadata(&self) -> &dyn DatabaseMetaData {
        &self.metadata
    }

    async fn get_catalog(&self) -> Result<Option<String>, SqlError> {
        let mut session = self.session.lock().await;
        let (_, rows) = session
            .query_all("select current_catalog", &[], true)
            .await?;
        Ok(rows
            .first()
            .and_then(|r| r.first())
            .and_then(SqlValue::to_java_string))
    }

    async fn get_schema(&self) -> Result<Option<String>, SqlError> {
        let mut session = self.session.lock().await;
        let (_, rows) = session
            .query_all("select current_schema()", &[], true)
            .await?;
        Ok(rows
            .first()
            .and_then(|r| r.first())
            .and_then(SqlValue::to_java_string))
    }

    async fn prepare_statement(&self, sql: &str) -> Result<PreparedStatement, SqlError> {
        if self.session.lock().await.is_closed() {
            return Err(super::error::connection_closed());
        }
        Ok(PreparedStatement::new(sql))
    }

    async fn execute_query(&self, statement: &PreparedStatement) -> Result<ResultSet, SqlError> {
        let native = native_sql(statement);
        check_parameters(&native, statement)?;
        let timer = self.query_timer(statement);
        let mut session = self.session.lock().await;
        let executed = session
            .execute(
                &native.sql,
                statement.parameters(),
                false,
                i32::try_from(statement.fetch_size()).unwrap_or(i32::MAX),
            )
            .await?;
        let (Some(fields), Some((id, buffer))) = (executed.fields, executed.result) else {
            return Err(psql("No results were returned by the query.", NO_DATA));
        };
        let metadata = Arc::new(result_metadata(session.types(), &fields));
        drop(session);
        Ok(ResultSet::new(
            metadata,
            Box::new(PgRowSource {
                session: Arc::clone(&self.session),
                id,
                buffer,
                timer,
            }),
        ))
    }

    async fn execute(&self, statement: &PreparedStatement) -> Result<bool, SqlError> {
        let native = native_sql(statement);
        check_parameters(&native, statement)?;
        let _timer = self.query_timer(statement);
        let mut session = self.session.lock().await;
        let executed = session
            .execute(&native.sql, statement.parameters(), false, 0)
            .await?;
        if let Some((id, _)) = executed.result {
            while session.next_row(id, true).await?.is_some() {}
        }
        Ok(executed.fields.is_some())
    }

    async fn statement_metadata(
        &self,
        statement: &PreparedStatement,
    ) -> Result<Option<Arc<ResultSetMetaData>>, SqlError> {
        let native = native_sql(statement);
        let mut session = self.session.lock().await;
        let described = session.describe(&native.sql).await?;
        let Some(fields) = described.fields else {
            return Ok(None);
        };
        let extra = self.field_metadata(&mut session, &fields).await?;
        let types = session.types();
        Ok(Some(Arc::new(ResultSetMetaData::new(
            fields
                .iter()
                .zip(&extra)
                .map(|(f, m)| column_metadata(types, f, m.as_ref()))
                .collect(),
        ))))
    }

    async fn parameter_count(
        &self,
        statement: &PreparedStatement,
    ) -> Result<Option<usize>, SqlError> {
        let native = native_sql(statement);
        let mut session = self.session.lock().await;
        let described = session.describe(&native.sql).await?;
        Ok(Some(described.parameter_types.len()))
    }

    async fn set_auto_commit(&self, auto_commit: bool) -> Result<(), SqlError> {
        self.session.lock().await.set_auto_commit(auto_commit).await
    }

    async fn get_auto_commit(&self) -> Result<bool, SqlError> {
        let session = self.session.lock().await;
        if session.is_closed() {
            return Err(super::error::connection_closed());
        }
        Ok(session.auto_commit())
    }

    async fn commit(&self) -> Result<(), SqlError> {
        self.session.lock().await.commit().await
    }

    async fn rollback(&self) -> Result<(), SqlError> {
        self.session.lock().await.rollback().await
    }

    async fn is_valid(&self, timeout_seconds: i32) -> Result<bool, SqlError> {
        if timeout_seconds < 0 {
            return Err(psql("Invalid timeout (<0).", DATA_TYPE_MISMATCH));
        }
        let mut session = self.session.lock().await;
        if session.is_closed() {
            return Ok(false);
        }
        let check = session.is_valid();
        if timeout_seconds == 0 {
            return Ok(check.await);
        }
        let limit = std::time::Duration::from_secs(u64::from(timeout_seconds.unsigned_abs()));
        Ok(tokio::time::timeout(limit, check).await.unwrap_or(false))
    }

    async fn close(&self) -> Result<(), SqlError> {
        self.session.lock().await.close().await;
        Ok(())
    }
}
