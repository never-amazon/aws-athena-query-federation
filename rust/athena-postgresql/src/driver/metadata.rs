use std::sync::Arc;

use async_trait::async_trait;
use athena_jdbc::sql::{
    ColumnMetadata, DatabaseMetaData, ResultSet, ResultSetMetaData, SqlError, SqlValue, Types,
};

use super::connection::{driver_column, result_metadata, varchar_column, SharedSession};
use super::types::{oid, TypeCache};

const SCHEMAS_QUERY: &str = "SELECT nspname AS \"TABLE_SCHEM\", current_database() AS \"TABLE_CATALOG\" FROM pg_catalog.pg_namespace  WHERE nspname <> 'pg_toast' AND (nspname !~ '^pg_temp_'  OR nspname = (pg_catalog.current_schemas(true))[1]) AND (nspname !~ '^pg_toast_temp_'  OR nspname = replace((pg_catalog.current_schemas(true))[1], 'pg_temp_', 'pg_toast_temp_')) ";

const TABLES_QUERY: &str = "SELECT current_database() AS \"TABLE_CAT\", n.nspname AS \"TABLE_SCHEM\", c.relname AS \"TABLE_NAME\",  CASE n.nspname ~ '^pg_' OR n.nspname = 'information_schema'  WHEN true THEN CASE  WHEN n.nspname = 'pg_catalog' OR n.nspname = 'information_schema' THEN CASE c.relkind   WHEN 'r' THEN 'SYSTEM TABLE'   WHEN 'v' THEN 'SYSTEM VIEW'   WHEN 'i' THEN 'SYSTEM INDEX'   ELSE NULL   END  WHEN n.nspname = 'pg_toast' THEN CASE c.relkind   WHEN 'r' THEN 'SYSTEM TOAST TABLE'   WHEN 'i' THEN 'SYSTEM TOAST INDEX'   ELSE NULL   END  ELSE CASE c.relkind   WHEN 'r' THEN 'TEMPORARY TABLE'   WHEN 'p' THEN 'TEMPORARY TABLE'   WHEN 'i' THEN 'TEMPORARY INDEX'   WHEN 'S' THEN 'TEMPORARY SEQUENCE'   WHEN 'v' THEN 'TEMPORARY VIEW'   ELSE NULL   END  END  WHEN false THEN CASE c.relkind  WHEN 'r' THEN 'TABLE'  WHEN 'p' THEN 'PARTITIONED TABLE'  WHEN 'i' THEN 'INDEX'  WHEN 'P' then 'PARTITIONED INDEX'  WHEN 'S' THEN 'SEQUENCE'  WHEN 'v' THEN 'VIEW'  WHEN 'c' THEN 'TYPE'  WHEN 'f' THEN 'FOREIGN TABLE'  WHEN 'm' THEN 'MATERIALIZED VIEW'  ELSE NULL  END  ELSE NULL  END  AS \"TABLE_TYPE\", d.description AS \"REMARKS\",  '' as \"TYPE_CAT\", '' as \"TYPE_SCHEM\", '' as \"TYPE_NAME\", '' AS \"SELF_REFERENCING_COL_NAME\", '' AS \"REF_GENERATION\"  FROM pg_catalog.pg_namespace n, pg_catalog.pg_class c  LEFT JOIN pg_catalog.pg_description d ON (c.oid = d.objoid AND d.objsubid = 0  and d.classoid = 'pg_class'::regclass)  WHERE c.relnamespace = n.oid ";

const TABLE_TYPE_CLAUSES: [(&str, &str); 14] = [
    (
        "TABLE",
        "c.relkind = 'r' AND n.nspname !~ '^pg_' AND n.nspname <> 'information_schema'",
    ),
    (
        "PARTITIONED TABLE",
        "c.relkind = 'p' AND n.nspname !~ '^pg_' AND n.nspname <> 'information_schema'",
    ),
    (
        "VIEW",
        "c.relkind = 'v' AND n.nspname <> 'pg_catalog' AND n.nspname <> 'information_schema'",
    ),
    (
        "INDEX",
        "c.relkind = 'i' AND n.nspname !~ '^pg_' AND n.nspname <> 'information_schema'",
    ),
    (
        "PARTITIONED INDEX",
        "c.relkind = 'I' AND n.nspname !~ '^pg_' AND n.nspname <> 'information_schema'",
    ),
    ("SEQUENCE", "c.relkind = 'S'"),
    (
        "TYPE",
        "c.relkind = 'c' AND n.nspname !~ '^pg_' AND n.nspname <> 'information_schema'",
    ),
    (
        "SYSTEM TABLE",
        "c.relkind = 'r' AND (n.nspname = 'pg_catalog' OR n.nspname = 'information_schema')",
    ),
    (
        "SYSTEM TOAST TABLE",
        "c.relkind = 'r' AND n.nspname = 'pg_toast'",
    ),
    (
        "SYSTEM TOAST INDEX",
        "c.relkind = 'i' AND n.nspname = 'pg_toast'",
    ),
    (
        "SYSTEM VIEW",
        "c.relkind = 'v' AND (n.nspname = 'pg_catalog' OR n.nspname = 'information_schema') ",
    ),
    (
        "SYSTEM INDEX",
        "c.relkind = 'i' AND (n.nspname = 'pg_catalog' OR n.nspname = 'information_schema') ",
    ),
    ("FOREIGN TABLE", "c.relkind = 'f'"),
    ("MATERIALIZED VIEW", "c.relkind = 'm'"),
];

const COLUMNS_QUERY: &str = "SELECT * FROM (SELECT current_database() AS current_database, n.nspname,c.relname,a.attname,a.atttypid,a.attnotnull  OR (t.typtype = 'd' AND t.typnotnull) AS attnotnull,a.atttypmod,a.attlen,t.typtypmod,row_number() OVER (PARTITION BY a.attrelid ORDER BY a.attnum) AS attnum, nullif(a.attidentity, '') as attidentity,nullif(a.attgenerated, '') as attgenerated,pg_catalog.pg_get_expr(def.adbin, def.adrelid) AS adsrc,dsc.description,t.typbasetype,t.typtype  FROM pg_catalog.pg_namespace n  JOIN pg_catalog.pg_class c ON (c.relnamespace = n.oid)  JOIN pg_catalog.pg_attribute a ON (a.attrelid=c.oid)  JOIN pg_catalog.pg_type t ON (a.atttypid = t.oid)  LEFT JOIN pg_catalog.pg_attrdef def ON (a.attrelid=def.adrelid AND a.attnum = def.adnum)  LEFT JOIN pg_catalog.pg_description dsc ON (c.oid=dsc.objoid AND a.attnum = dsc.objsubid)  LEFT JOIN pg_catalog.pg_class dc ON (dc.oid=dsc.classoid AND dc.relname='pg_class')  LEFT JOIN pg_catalog.pg_namespace dn ON (dc.relnamespace=dn.oid AND dn.nspname='pg_catalog')  WHERE c.relkind in ('r','p','v','f','m') and a.attnum > 0 AND NOT a.attisdropped";

const PRIMARY_KEYS_QUERY_START: &str = "SELECT        result.TABLE_CAT AS \"TABLE_CAT\",        result.TABLE_SCHEM AS \"TABLE_SCHEM\",        result.TABLE_NAME AS \"TABLE_NAME\",        result.COLUMN_NAME AS \"COLUMN_NAME\",        result.KEY_SEQ AS \"KEY_SEQ\",        result.PK_NAME AS \"PK_NAME\"FROM      (SELECT current_database() AS TABLE_CAT, n.nspname AS TABLE_SCHEM,   ct.relname AS TABLE_NAME, a.attname AS COLUMN_NAME,   (information_schema._pg_expandarray(con.conkey)).n AS KEY_SEQ, con.conname AS PK_NAME,   information_schema._pg_expandarray(con.conkey) AS KEYS, a.attnum AS A_ATTNUM FROM pg_catalog.pg_constraint con   JOIN pg_catalog.pg_class ct ON (con.conrelid = ct.oid)   JOIN pg_catalog.pg_namespace n ON (ct.relnamespace = n.oid)   JOIN pg_catalog.pg_attribute a ON (a.attrelid = ct.oid) WHERE con.contype = 'p' ";

const PRIMARY_KEYS_QUERY_END: &str = " ) result where  result.A_ATTNUM = (result.KEYS).x  ORDER BY result.table_name, result.pk_name, result.key_seq";

const CHARACTER_SQL_TYPES: [i32; 12] = [
    Types::LONGNVARCHAR,
    Types::NCHAR,
    Types::NVARCHAR,
    Types::LONGVARBINARY,
    Types::VARBINARY,
    Types::BINARY,
    Types::LONGVARCHAR,
    Types::CHAR,
    Types::VARCHAR,
    Types::BLOB,
    Types::CLOB,
    Types::NCLOB,
];

pub struct PgDatabaseMetaData {
    session: SharedSession,
}

fn present(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.is_empty())
}

fn string_or_null(value: Option<String>) -> SqlValue {
    value.map_or(SqlValue::Null, SqlValue::String)
}

fn int_column(name: &str) -> ColumnMetadata {
    driver_column(name, Types::INTEGER, "int4")
}

fn columns_metadata() -> ResultSetMetaData {
    let smallint = |name: &str| driver_column(name, Types::SMALLINT, "int2");
    ResultSetMetaData::new(vec![
        varchar_column("TABLE_CAT"),
        varchar_column("TABLE_SCHEM"),
        varchar_column("TABLE_NAME"),
        varchar_column("COLUMN_NAME"),
        smallint("DATA_TYPE"),
        varchar_column("TYPE_NAME"),
        int_column("COLUMN_SIZE"),
        varchar_column("BUFFER_LENGTH"),
        int_column("DECIMAL_DIGITS"),
        int_column("NUM_PREC_RADIX"),
        int_column("NULLABLE"),
        varchar_column("REMARKS"),
        varchar_column("COLUMN_DEF"),
        int_column("SQL_DATA_TYPE"),
        int_column("SQL_DATETIME_SUB"),
        int_column("CHAR_OCTET_LENGTH"),
        int_column("ORDINAL_POSITION"),
        varchar_column("IS_NULLABLE"),
        varchar_column("SCOPE_CATALOG"),
        varchar_column("SCOPE_SCHEMA"),
        varchar_column("SCOPE_TABLE"),
        smallint("SOURCE_DATA_TYPE"),
        varchar_column("IS_AUTOINCREMENT"),
        varchar_column("IS_GENERATEDCOLUMN"),
    ])
}

fn text(row: &[SqlValue], i: usize) -> Option<String> {
    row.get(i).and_then(SqlValue::to_java_string)
}

fn number(row: &[SqlValue], i: usize) -> i64 {
    row.get(i)
        .and_then(|v| v.to_long().ok().flatten())
        .unwrap_or(0)
}

fn column_row(types: &TypeCache, row: &[SqlValue]) -> Vec<SqlValue> {
    let type_oid = u32::try_from(number(row, 4)).unwrap_or(0);
    let type_mod = i32::try_from(number(row, 6)).unwrap_or(-1);
    let type_type = text(row, 15).unwrap_or_default();
    let sql_type = match type_type.as_str() {
        "c" => Types::STRUCT,
        "d" => Types::DISTINCT,
        "e" => Types::VARCHAR,
        _ => types.sql_type(type_oid),
    };
    let mut pg_type = types.pg_type_name(type_oid);
    let default_value = text(row, 12);
    let uses_sequence = default_value
        .as_deref()
        .is_some_and(|d| d.contains("nextval("));
    if uses_sequence {
        pg_type = match pg_type.as_str() {
            "int4" => "serial".to_string(),
            "int8" => "bigserial".to_string(),
            "int2" => "smallserial".to_string(),
            _ => pg_type,
        };
    }
    let identity = text(row, 10);
    let generated = text(row, 11);
    let base_type_oid = u32::try_from(number(row, 14)).unwrap_or(0);
    let (decimal_digits, column_size) = if sql_type == Types::DISTINCT {
        let type_type_mod = i32::try_from(number(row, 8)).unwrap_or(-1);
        if type_type_mod == -1 {
            (
                types.scale(base_type_oid, type_mod),
                types.precision(base_type_oid, type_mod),
            )
        } else if base_type_oid == oid::NUMERIC {
            (
                types.scale(base_type_oid, type_type_mod),
                types.precision(base_type_oid, type_type_mod),
            )
        } else {
            (types.scale(base_type_oid, type_mod), type_type_mod)
        }
    } else {
        let size = types.precision(type_oid, type_mod);
        let size = if sql_type != Types::NUMERIC && size == 0 {
            types.display_size(type_oid, type_mod)
        } else {
            size
        };
        (types.scale(type_oid, type_mod), size)
    };
    let decimal_digits =
        if (sql_type == Types::NUMERIC || sql_type == Types::DECIMAL) && type_mod == -1 {
            SqlValue::Null
        } else {
            SqlValue::Integer(decimal_digits)
        };
    let radix = if pg_type == "bit" || pg_type == "varbit" {
        2
    } else {
        10
    };
    let not_null = text(row, 5).as_deref() == Some("t");
    let char_octet_length = if CHARACTER_SQL_TYPES.contains(&sql_type) {
        SqlValue::Integer(column_size)
    } else {
        SqlValue::Null
    };
    let source_data_type = if sql_type == Types::DISTINCT {
        i16::try_from(types.sql_type(base_type_oid)).map_or(SqlValue::Null, SqlValue::SmallInt)
    } else {
        SqlValue::Null
    };
    let auto_increment = uses_sequence || identity.is_some();
    vec![
        string_or_null(text(row, 0)),
        string_or_null(text(row, 1)),
        string_or_null(text(row, 2)),
        string_or_null(text(row, 3)),
        i16::try_from(sql_type).map_or(SqlValue::Integer(sql_type), SqlValue::SmallInt),
        SqlValue::String(pg_type),
        SqlValue::Integer(column_size),
        SqlValue::Null,
        decimal_digits,
        SqlValue::Integer(radix),
        SqlValue::Integer(i32::from(!not_null)),
        string_or_null(text(row, 13)),
        string_or_null(default_value),
        SqlValue::Null,
        SqlValue::Null,
        char_octet_length,
        SqlValue::Integer(i32::try_from(number(row, 9)).unwrap_or(0)),
        SqlValue::String(if not_null { "NO" } else { "YES" }.to_string()),
        SqlValue::Null,
        SqlValue::Null,
        SqlValue::Null,
        source_data_type,
        SqlValue::String(if auto_increment { "YES" } else { "NO" }.to_string()),
        SqlValue::String(if generated.is_some() { "YES" } else { "NO" }.to_string()),
    ]
}

impl PgDatabaseMetaData {
    pub(crate) fn new(session: SharedSession) -> PgDatabaseMetaData {
        PgDatabaseMetaData { session }
    }

    async fn query(&self, sql: &str, parameters: Vec<SqlValue>) -> Result<ResultSet, SqlError> {
        let mut session = self.session.lock().await;
        let (fields, rows) = session.query_all(sql, &parameters, false).await?;
        let metadata = result_metadata(session.types(), &fields);
        Ok(ResultSet::new(
            Arc::new(metadata),
            Box::new(athena_jdbc::sql::VecRowSource::new(rows)),
        ))
    }
}

#[async_trait]
impl DatabaseMetaData for PgDatabaseMetaData {
    async fn get_schemas(&self) -> Result<ResultSet, SqlError> {
        self.get_schemas_with_catalog(None, None).await
    }

    async fn get_schemas_with_catalog(
        &self,
        _catalog: Option<&str>,
        schema_pattern: Option<&str>,
    ) -> Result<ResultSet, SqlError> {
        let mut sql = SCHEMAS_QUERY.to_string();
        let mut parameters = Vec::new();
        if let Some(pattern) = present(schema_pattern) {
            sql.push_str(" AND nspname LIKE ?");
            parameters.push(SqlValue::String(pattern.to_string()));
        }
        sql.push_str(" ORDER BY \"TABLE_SCHEM\"");
        self.query(&super::parser::to_native_sql(&sql).sql, parameters)
            .await
    }

    async fn get_tables(
        &self,
        _catalog: Option<&str>,
        schema_pattern: Option<&str>,
        table_name_pattern: Option<&str>,
        types: Option<&[&str]>,
    ) -> Result<ResultSet, SqlError> {
        let mut sql = TABLES_QUERY.to_string();
        let mut parameters = Vec::new();
        if let Some(pattern) = present(schema_pattern) {
            sql.push_str(" AND n.nspname LIKE ?");
            parameters.push(SqlValue::String(pattern.to_string()));
        }
        if let Some(pattern) = present(table_name_pattern) {
            sql.push_str(" AND c.relname LIKE ?");
            parameters.push(SqlValue::String(pattern.to_string()));
        }
        if let Some(types) = types {
            sql.push_str(" AND (false ");
            for table_type in types {
                if let Some((_, clause)) = TABLE_TYPE_CLAUSES.iter().find(|(t, _)| t == table_type)
                {
                    sql.push_str(" OR ( ");
                    sql.push_str(clause);
                    sql.push_str(" ) ");
                }
            }
            sql.push_str(") ");
        }
        sql.push_str(" ORDER BY \"TABLE_TYPE\",\"TABLE_SCHEM\",\"TABLE_NAME\" ");
        self.query(&super::parser::to_native_sql(&sql).sql, parameters)
            .await
    }

    async fn get_columns(
        &self,
        catalog: Option<&str>,
        schema_pattern: Option<&str>,
        table_name_pattern: Option<&str>,
        column_name_pattern: Option<&str>,
    ) -> Result<ResultSet, SqlError> {
        let mut sql = COLUMNS_QUERY.to_string();
        let mut parameters = Vec::new();
        if let Some(pattern) = present(schema_pattern) {
            sql.push_str(" AND n.nspname LIKE ?");
            parameters.push(SqlValue::String(pattern.to_string()));
        }
        if let Some(pattern) = present(table_name_pattern) {
            sql.push_str(" AND c.relname LIKE ?");
            parameters.push(SqlValue::String(pattern.to_string()));
        }
        sql.push_str(") c WHERE true ");
        if let Some(pattern) = present(column_name_pattern) {
            sql.push_str(" AND attname LIKE ?");
            parameters.push(SqlValue::String(pattern.to_string()));
        }
        sql.push_str(" ORDER BY nspname,c.relname,attnum ");
        let native = super::parser::to_native_sql(&sql).sql;
        let mut session = self.session.lock().await;
        let (_, rows) = session.query_all(&native, &parameters, false).await?;
        let oids: Vec<u32> = rows
            .iter()
            .flat_map(|r| [number(r, 4), number(r, 14)])
            .filter_map(|o| u32::try_from(o).ok())
            .filter(|o| *o != 0)
            .collect();
        session.resolve_types(oids).await?;
        let types = session.types();
        let rows: Vec<Vec<SqlValue>> = rows
            .iter()
            .filter(|r| catalog.is_none_or(|c| text(r, 0).as_deref() == Some(c)))
            .map(|r| column_row(types, r))
            .collect();
        Ok(ResultSet::from_rows(columns_metadata(), rows))
    }

    async fn get_primary_keys(
        &self,
        _catalog: Option<&str>,
        schema: Option<&str>,
        table: &str,
    ) -> Result<ResultSet, SqlError> {
        let mut sql = PRIMARY_KEYS_QUERY_START.to_string();
        let mut parameters = Vec::new();
        if let Some(schema) = present(schema) {
            sql.push_str(" AND n.nspname = ?");
            parameters.push(SqlValue::String(schema.to_string()));
        }
        if !table.is_empty() {
            sql.push_str(" AND ct.relname = ?");
            parameters.push(SqlValue::String(table.to_string()));
        }
        sql.push_str(PRIMARY_KEYS_QUERY_END);
        self.query(&super::parser::to_native_sql(&sql).sql, parameters)
            .await
    }

    fn get_search_string_escape(&self) -> Result<Option<String>, SqlError> {
        Ok(Some("\\".to_string()))
    }

    fn get_identifier_quote_string(&self) -> Result<String, SqlError> {
        Ok("\"".to_string())
    }

    fn get_database_product_name(&self) -> Result<String, SqlError> {
        Ok("PostgreSQL".to_string())
    }

    async fn get_user_name(&self) -> Result<Option<String>, SqlError> {
        Ok(self.session.lock().await.user().map(ToString::to_string))
    }
}
