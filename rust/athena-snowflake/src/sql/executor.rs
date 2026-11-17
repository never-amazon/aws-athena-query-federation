use std::str::FromStr;
use std::time::Duration;

use async_trait::async_trait;
use athena_jdbc::sql::{RowSource, SqlException, SqlValue};
use bigdecimal::BigDecimal;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use snowflake_connector_rs::bind::{Binary, BindType, RawBind, Time, TimestampLtz, TimestampTz};
use snowflake_connector_rs::decode::{CellPlan, FromRow, RowPlanContext};
use snowflake_connector_rs::error::{PlanBuildError, RowDecodeError};
use snowflake_connector_rs::{
    BinaryValue, ColumnType, QueryOptions, ResultCursor, RowRef, Session, Statement,
};

use super::type_map::{SnowflakeColumn, SnowflakeColumnType};

pub const SNOWFLAKE_SQL_EXCEPTION: &str =
    "net.snowflake.client.api.exception.SnowflakeSQLException";
const NOT_FOUND_ERROR_CODE: &str = "002003";
const BASE_TABLE_OR_VIEW_NOT_FOUND: &str = "42S02";

pub struct SnowflakeQueryResult {
    pub columns: Vec<SnowflakeColumn>,
    pub rows: Box<dyn RowSource>,
}

#[async_trait]
pub trait SnowflakeExecutor: Send + Sync {
    async fn query(
        &self,
        sql: &str,
        binds: &[SqlValue],
        timeout: Option<Duration>,
    ) -> Result<SnowflakeQueryResult, SqlException>;

    async fn close(&self) -> Result<(), SqlException> {
        Ok(())
    }
}

pub(crate) fn snowflake_exception(message: impl Into<String>) -> SqlException {
    SqlException::with_class_name(SNOWFLAKE_SQL_EXCEPTION, message)
}

pub(crate) fn from_connector_error(error: &snowflake_connector_rs::Error) -> SqlException {
    let message = error
        .snowflake_message()
        .map_or_else(|| error.to_string(), ToString::to_string);
    let mut exception = snowflake_exception(message);
    if let Some(code) = error.snowflake_code() {
        if let Ok(number) = code.parse::<i32>() {
            exception = exception.with_error_code(number);
        }
        if code == NOT_FOUND_ERROR_CODE {
            exception = exception.with_sql_state(BASE_TABLE_OR_VIEW_NOT_FOUND);
        }
    }
    exception
}

pub(crate) fn column_type_of(ty: &ColumnType) -> SnowflakeColumnType {
    let precision = i32::from(ty.precision().unwrap_or(0));
    let scale = i32::from(ty.scale().unwrap_or(0));
    let length = ty
        .length()
        .map_or(0, |l| i32::try_from(l).unwrap_or(i32::MAX));
    let timestamp_scale = i32::from(ty.scale().unwrap_or(9));
    match ty.as_str() {
        "fixed" => SnowflakeColumnType::Fixed { precision, scale },
        "real" => SnowflakeColumnType::Real,
        "text" => SnowflakeColumnType::Text { length },
        "boolean" => SnowflakeColumnType::Boolean,
        "date" => SnowflakeColumnType::Date,
        "time" => SnowflakeColumnType::Time {
            scale: timestamp_scale,
        },
        "timestamp_ntz" => SnowflakeColumnType::TimestampNtz {
            scale: timestamp_scale,
        },
        "timestamp_ltz" => SnowflakeColumnType::TimestampLtz {
            scale: timestamp_scale,
        },
        "timestamp_tz" => SnowflakeColumnType::TimestampTz {
            scale: timestamp_scale,
        },
        "variant" => SnowflakeColumnType::Variant,
        "object" => SnowflakeColumnType::Object,
        "array" => SnowflakeColumnType::Array,
        "binary" => SnowflakeColumnType::Binary { length },
        "geography" => SnowflakeColumnType::Geography,
        "geometry" => SnowflakeColumnType::Geometry,
        "vector" => SnowflakeColumnType::Vector,
        other => SnowflakeColumnType::Unknown(other.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RawCell {
    Null,
    Text(String),
    Date(NaiveDate),
    Time(NaiveTime),
    TimestampNtz(NaiveDateTime),
    TimestampLtz(DateTime<Utc>),
    TimestampTz(DateTime<FixedOffset>),
    Binary(Vec<u8>),
}

enum CellDecoder {
    Raw(usize),
    Date(CellPlan<Option<NaiveDate>>),
    Time(CellPlan<Option<NaiveTime>>),
    TimestampNtz(CellPlan<Option<NaiveDateTime>>),
    TimestampLtz(CellPlan<Option<DateTime<Utc>>>),
    TimestampTz(CellPlan<Option<DateTime<FixedOffset>>>),
    Binary(CellPlan<Option<BinaryValue>>),
}

struct RawRow(Vec<RawCell>);

impl FromRow for RawRow {
    type Plan = Vec<CellDecoder>;

    fn build_plan(ctx: RowPlanContext<'_>) -> Result<Self::Plan, PlanBuildError> {
        let columns = ctx.schema().columns().to_vec();
        let mut plan = Vec::with_capacity(columns.len());
        for (position, column) in columns.iter().enumerate() {
            let decoder = match column.ty().as_str() {
                "date" => CellDecoder::Date(CellPlan::by_position(ctx, position)?),
                "time" => CellDecoder::Time(CellPlan::by_position(ctx, position)?),
                "timestamp_ntz" => CellDecoder::TimestampNtz(CellPlan::by_position(ctx, position)?),
                "timestamp_ltz" => CellDecoder::TimestampLtz(CellPlan::by_position(ctx, position)?),
                "timestamp_tz" => CellDecoder::TimestampTz(CellPlan::by_position(ctx, position)?),
                "binary" => CellDecoder::Binary(CellPlan::by_position(ctx, position)?),
                _ => CellDecoder::Raw(position),
            };
            plan.push(decoder);
        }
        Ok(plan)
    }

    fn from_row_with_plan(row: RowRef<'_>, plan: &Self::Plan) -> Result<Self, RowDecodeError> {
        let mut cells = Vec::with_capacity(plan.len());
        for decoder in plan {
            let cell = match decoder {
                CellDecoder::Raw(position) => row
                    .cell_at(*position)?
                    .raw()
                    .map_or(RawCell::Null, |raw| RawCell::Text(raw.to_string())),
                CellDecoder::Date(p) => row.get_with_plan(p)?.map_or(RawCell::Null, RawCell::Date),
                CellDecoder::Time(p) => row.get_with_plan(p)?.map_or(RawCell::Null, RawCell::Time),
                CellDecoder::TimestampNtz(p) => row
                    .get_with_plan(p)?
                    .map_or(RawCell::Null, RawCell::TimestampNtz),
                CellDecoder::TimestampLtz(p) => row
                    .get_with_plan(p)?
                    .map_or(RawCell::Null, RawCell::TimestampLtz),
                CellDecoder::TimestampTz(p) => row
                    .get_with_plan(p)?
                    .map_or(RawCell::Null, RawCell::TimestampTz),
                CellDecoder::Binary(p) => row
                    .get_with_plan(p)?
                    .map_or(RawCell::Null, |b| RawCell::Binary(b.as_bytes().to_vec())),
            };
            cells.push(cell);
        }
        Ok(RawRow(cells))
    }
}

fn decode_error(column: &SnowflakeColumn, raw: &str) -> SqlException {
    snowflake_exception(format!(
        "Cannot convert value in the driver from type:{} to type:{}, value={raw}.",
        column.column_type.wire_name().to_uppercase(),
        column.column_type.wire_name().to_uppercase()
    ))
}

pub(crate) fn decode_cell(
    column: &SnowflakeColumn,
    cell: RawCell,
) -> Result<SqlValue, SqlException> {
    let raw = match cell {
        RawCell::Null => return Ok(SqlValue::Null),
        RawCell::Date(d) => return Ok(SqlValue::Date(d)),
        RawCell::Time(t) => return Ok(SqlValue::Time(t)),
        RawCell::TimestampNtz(ts) => return Ok(SqlValue::Timestamp(ts)),
        RawCell::TimestampLtz(ts) => return Ok(SqlValue::Timestamp(ts.naive_utc())),
        RawCell::TimestampTz(ts) => return Ok(SqlValue::TimestampTz(ts)),
        RawCell::Binary(bytes) => return Ok(SqlValue::Bytes(bytes)),
        RawCell::Text(raw) => raw,
    };
    match &column.column_type {
        SnowflakeColumnType::Fixed { scale, .. } => {
            if *scale == 0 {
                if let Ok(value) = raw.parse::<i64>() {
                    return Ok(SqlValue::BigInt(value));
                }
            }
            BigDecimal::from_str(&raw)
                .map(SqlValue::Decimal)
                .map_err(|_| decode_error(column, &raw))
        }
        SnowflakeColumnType::Real => parse_real(&raw)
            .map(SqlValue::Double)
            .ok_or_else(|| decode_error(column, &raw)),
        SnowflakeColumnType::Boolean => {
            if raw == "1" || raw.eq_ignore_ascii_case("true") {
                Ok(SqlValue::Boolean(true))
            } else if raw == "0" || raw.eq_ignore_ascii_case("false") {
                Ok(SqlValue::Boolean(false))
            } else {
                Err(decode_error(column, &raw))
            }
        }
        _ => Ok(SqlValue::String(raw)),
    }
}

fn parse_real(raw: &str) -> Option<f64> {
    match raw {
        "inf" | "Infinity" => Some(f64::INFINITY),
        "-inf" | "-Infinity" => Some(f64::NEG_INFINITY),
        "NaN" | "nan" => Some(f64::NAN),
        other => other.parse::<f64>().ok(),
    }
}

pub(crate) fn decode_row(
    columns: &[SnowflakeColumn],
    cells: Vec<RawCell>,
) -> Result<Vec<SqlValue>, SqlException> {
    columns
        .iter()
        .zip(cells)
        .map(|(column, cell)| decode_cell(column, cell))
        .collect()
}

struct CursorRowSource {
    cursor: ResultCursor,
    columns: Vec<SnowflakeColumn>,
    buffered: std::vec::IntoIter<Vec<RawCell>>,
}

#[async_trait]
impl RowSource for CursorRowSource {
    async fn next_row(&mut self) -> Result<Option<Vec<SqlValue>>, SqlException> {
        loop {
            if let Some(cells) = self.buffered.next() {
                return decode_row(&self.columns, cells).map(Some);
            }
            let Some(table) = self
                .cursor
                .next_table()
                .await
                .map_err(|e| from_connector_error(&e))?
            else {
                return Ok(None);
            };
            let rows = table
                .rows::<RawRow>()
                .map_err(|e| from_connector_error(&e))?
                .map(|row| row.map(|r| r.0))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| from_connector_error(&e))?;
            self.buffered = rows.into_iter();
        }
    }
}

fn bind_error(error: impl std::fmt::Display) -> SqlException {
    snowflake_exception(error.to_string())
}

fn bind_statement(
    sql: &str,
    binds: &[SqlValue],
) -> Result<Statement<snowflake_connector_rs::PositionalBinds>, SqlException> {
    let mut statement = Statement::new_positional(sql.to_string());
    for value in binds {
        statement = match value {
            SqlValue::Null => statement.bind(Option::<String>::None),
            SqlValue::Boolean(v) => statement.bind(*v),
            SqlValue::TinyInt(v) => statement.bind(*v),
            SqlValue::SmallInt(v) => statement.bind(*v),
            SqlValue::Integer(v) => statement.bind(*v),
            SqlValue::BigInt(v) => statement.bind(*v),
            SqlValue::Real(v) => statement.bind(*v),
            SqlValue::Double(v) => statement.bind(*v),
            SqlValue::Decimal(v) => statement.bind(RawBind::new(BindType::Fixed, v.to_string())),
            SqlValue::String(v) => statement.bind(v.clone()),
            SqlValue::Bytes(v) => statement.bind(Binary::new(v.clone())),
            SqlValue::Date(v) => statement.bind(*v),
            SqlValue::Time(v) => statement.bind(Time::try_from(*v).map_err(bind_error)?),
            SqlValue::Timestamp(v) => {
                statement.bind(TimestampLtz::try_from(v.and_utc()).map_err(bind_error)?)
            }
            SqlValue::TimestampTz(v) => {
                statement.bind(TimestampTz::try_from(*v).map_err(bind_error)?)
            }
            SqlValue::Array(_) => {
                return Err(snowflake_exception(
                    "Data type ARRAY is not supported for binding",
                ))
            }
        };
    }
    Ok(statement)
}

pub struct ConnectorExecutor {
    session: Session,
}

impl ConnectorExecutor {
    pub fn new(session: Session) -> ConnectorExecutor {
        ConnectorExecutor { session }
    }
}

#[async_trait]
impl SnowflakeExecutor for ConnectorExecutor {
    async fn query(
        &self,
        sql: &str,
        binds: &[SqlValue],
        timeout: Option<Duration>,
    ) -> Result<SnowflakeQueryResult, SqlException> {
        let statement = bind_statement(sql, binds)?;
        let mut options = QueryOptions::new();
        if let Some(timeout) = timeout {
            options = options.with_query_response_timeout(timeout);
        }
        let cursor = self
            .session
            .query_with_options(statement, options)
            .await
            .map_err(|e| from_connector_error(&e))?;
        let columns = cursor
            .schema()
            .columns()
            .iter()
            .map(|column| {
                SnowflakeColumn::new(
                    column.name(),
                    column_type_of(column.ty()),
                    column.is_nullable(),
                )
            })
            .collect();
        let rows = Box::new(CursorRowSource {
            cursor,
            columns: Vec::clone(&columns),
            buffered: Vec::new().into_iter(),
        });
        Ok(SnowflakeQueryResult { columns, rows })
    }
}
