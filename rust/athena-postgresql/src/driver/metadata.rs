use async_trait::async_trait;
use athena_jdbc::sql::{
    ColumnInfo, DatabaseMetaData, JdbcType, SchemaEntry, SqlError, SqlValue, TableEntry,
};

use super::connection::SharedSession;
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

const CURRENT_DATABASE: usize = 0;
const ATTNAME: usize = 3;
const ATTTYPID: usize = 4;
const ATTTYPMOD: usize = 6;
const TYPTYPMOD: usize = 8;
const ATTNUM: usize = 9;
const ADSRC: usize = 12;
const TYPBASETYPE: usize = 14;
const TYPTYPE: usize = 15;

pub struct PgDatabaseMetaData {
    session: SharedSession,
}

fn present(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.is_empty())
}

fn text(row: &[SqlValue], i: usize) -> Option<String> {
    row.get(i).and_then(SqlValue::to_java_string)
}

fn number(row: &[SqlValue], i: usize) -> i64 {
    row.get(i)
        .and_then(|v| v.to_long().ok().flatten())
        .unwrap_or(0)
}

fn column_info(types: &TypeCache, row: &[SqlValue]) -> ColumnInfo {
    let type_oid = u32::try_from(number(row, ATTTYPID)).unwrap_or(0);
    let type_mod = i32::try_from(number(row, ATTTYPMOD)).unwrap_or(-1);
    let type_type = text(row, TYPTYPE).unwrap_or_default();
    let sql_type = match type_type.as_str() {
        "c" => JdbcType::Struct,
        "d" => JdbcType::Distinct,
        "e" => JdbcType::Varchar,
        _ => types.sql_type(type_oid),
    };
    let mut pg_type = types.pg_type_name(type_oid);
    let default_value = text(row, ADSRC);
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
    let base_type_oid = u32::try_from(number(row, TYPBASETYPE)).unwrap_or(0);
    let (decimal_digits, column_size) = if sql_type == JdbcType::Distinct {
        let type_type_mod = i32::try_from(number(row, TYPTYPMOD)).unwrap_or(-1);
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
        let size = if sql_type != JdbcType::Numeric && size == 0 {
            types.display_size(type_oid, type_mod)
        } else {
            size
        };
        (types.scale(type_oid, type_mod), size)
    };
    let decimal_digits = ((sql_type != JdbcType::Numeric && sql_type != JdbcType::Decimal)
        || type_mod != -1)
        .then_some(decimal_digits);
    ColumnInfo {
        name: text(row, ATTNAME),
        data_type: sql_type,
        type_name: Some(pg_type),
        column_size: Some(column_size),
        decimal_digits,
        ordinal_position: i32::try_from(number(row, ATTNUM)).unwrap_or(0),
    }
}

impl PgDatabaseMetaData {
    pub(crate) fn new(session: SharedSession) -> PgDatabaseMetaData {
        PgDatabaseMetaData { session }
    }

    async fn query(
        &self,
        sql: &str,
        parameters: Vec<SqlValue>,
    ) -> Result<Vec<Vec<SqlValue>>, SqlError> {
        let mut session = self.session.lock().await;
        let (_, rows) = session.query_all(sql, &parameters, false).await?;
        Ok(rows)
    }
}

#[async_trait]
impl DatabaseMetaData for PgDatabaseMetaData {
    async fn schemas(
        &self,
        _catalog: Option<&str>,
        schema_pattern: Option<&str>,
    ) -> Result<Vec<SchemaEntry>, SqlError> {
        let mut sql = SCHEMAS_QUERY.to_string();
        let mut parameters = Vec::new();
        if let Some(pattern) = present(schema_pattern) {
            sql.push_str(" AND nspname LIKE ?");
            parameters.push(SqlValue::String(pattern.to_string()));
        }
        sql.push_str(" ORDER BY \"TABLE_SCHEM\"");
        let rows = self
            .query(&super::parser::to_native_sql(&sql).sql, parameters)
            .await?;
        Ok(rows
            .iter()
            .map(|row| SchemaEntry {
                schema: text(row, 0),
                catalog: text(row, 1),
            })
            .collect())
    }

    async fn tables(
        &self,
        _catalog: Option<&str>,
        schema_pattern: Option<&str>,
        table_name_pattern: Option<&str>,
        types: Option<&[&str]>,
    ) -> Result<Vec<TableEntry>, SqlError> {
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
        let rows = self
            .query(&super::parser::to_native_sql(&sql).sql, parameters)
            .await?;
        Ok(rows
            .iter()
            .map(|row| TableEntry {
                catalog: text(row, 0),
                schema: text(row, 1),
                name: text(row, 2).unwrap_or_default(),
                kind: text(row, 3),
                remarks: text(row, 4),
            })
            .collect())
    }

    async fn columns(
        &self,
        catalog: Option<&str>,
        schema_pattern: Option<&str>,
        table_name_pattern: Option<&str>,
        column_name_pattern: Option<&str>,
    ) -> Result<Vec<ColumnInfo>, SqlError> {
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
            .flat_map(|r| [number(r, ATTTYPID), number(r, TYPBASETYPE)])
            .filter_map(|o| u32::try_from(o).ok())
            .filter(|o| *o != 0)
            .collect();
        session.resolve_types(oids).await?;
        let types = session.types();
        Ok(rows
            .iter()
            .filter(|r| catalog.is_none_or(|c| text(r, CURRENT_DATABASE).as_deref() == Some(c)))
            .map(|r| column_info(types, r))
            .collect())
    }

    async fn primary_key_columns(
        &self,
        _catalog: Option<&str>,
        schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<Option<String>>, SqlError> {
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
        let rows = self
            .query(&super::parser::to_native_sql(&sql).sql, parameters)
            .await?;
        Ok(rows.iter().map(|row| text(row, 3)).collect())
    }

    fn search_string_escape(&self) -> Option<String> {
        Some("\\".to_string())
    }

    fn database_product_name(&self) -> String {
        "PostgreSQL".to_string()
    }
}
