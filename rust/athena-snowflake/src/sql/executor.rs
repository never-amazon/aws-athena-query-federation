use std::future::Future;
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

use super::hybrid_calendar::{java_sql_date_label, java_sql_timestamp, DateLabel};
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
        RawCell::Date(d) => return Ok(date_value(d)),
        RawCell::Time(t) => return Ok(SqlValue::Time(t)),
        RawCell::TimestampNtz(ts) => return Ok(timestamp_value(ts)),
        RawCell::TimestampLtz(ts) => return Ok(timestamp_value(ts.naive_utc())),
        RawCell::TimestampTz(ts) => return Ok(timestamp_tz_value(ts)),
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

fn date_value(date: NaiveDate) -> SqlValue {
    let text = SqlValue::Date(date).to_string();
    match java_sql_date_label(date) {
        DateLabel::Same => SqlValue::Date(date),
        DateLabel::Shifted(label) => SqlValue::rendered(SqlValue::Date(label), text),
        DateLabel::Unparseable(label) => SqlValue::rendered(SqlValue::String(label), text),
    }
}

fn timestamp_value(timestamp: NaiveDateTime) -> SqlValue {
    let adjusted = java_sql_timestamp(timestamp);
    if adjusted == timestamp {
        return SqlValue::Timestamp(timestamp);
    }
    let text = SqlValue::Timestamp(timestamp).to_string();
    SqlValue::rendered(SqlValue::Timestamp(adjusted), text)
}

fn timestamp_tz_value(timestamp: DateTime<FixedOffset>) -> SqlValue {
    let adjusted = java_sql_timestamp(timestamp.naive_utc());
    if adjusted == timestamp.naive_utc() {
        return SqlValue::TimestampTz(timestamp);
    }
    let text = SqlValue::TimestampTz(timestamp).to_string();
    let adjusted = adjusted.and_utc().with_timezone(timestamp.offset());
    SqlValue::rendered(SqlValue::TimestampTz(adjusted), text)
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
        let mut value = value;
        while let SqlValue::Rendered { value: inner, .. } = value {
            value = inner.as_ref();
        }
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
            SqlValue::Rendered { .. } => unreachable!("rendered values are unwrapped above"),
        };
    }
    Ok(statement)
}

const CANCEL_GRACE_PERIOD: Duration = Duration::from_secs(30);

pub(crate) async fn run_with_query_timeout<T, Cancel, CancelFuture>(
    execution: impl Future<Output = T>,
    timeout: Option<Duration>,
    cancel: Cancel,
) -> T
where
    Cancel: FnOnce() -> CancelFuture,
    CancelFuture: Future<Output = ()>,
{
    let Some(timeout) = timeout else {
        return execution.await;
    };
    let mut execution = std::pin::pin!(execution);
    tokio::select! {
        result = &mut execution => result,
        () = tokio::time::sleep(timeout) => {
            cancel().await;
            execution.await
        }
    }
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
            options = options.with_query_response_timeout(timeout + CANCEL_GRACE_PERIOD);
        }
        let handle = self
            .session
            .query_handle_with_options(statement, options)
            .map_err(|e| from_connector_error(&e))?;
        let canceller = handle.canceller();
        let cursor = run_with_query_timeout(handle.execute(), timeout, || async move {
            if let Err(error) = canceller.cancel().await {
                tracing::warn!("Snowflake query cancel failed: {error}");
            }
        })
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

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Days, FixedOffset, NaiveDate, NaiveDateTime};

    use super::{decode_cell, RawCell, SnowflakeColumn, SnowflakeColumnType, SqlValue};

    const JAVA_DATE_EPOCH_DAYS: [(i64, Option<i64>); 21] = [
        (0, Some(0)),
        (2_932_896, Some(2_932_896)),
        (-141_427, Some(-141_427)),
        (-141_426, Some(-141_426)),
        (-141_428, Some(-141_418)),
        (-141_429, Some(-141_419)),
        (-141_436, Some(-141_426)),
        (-141_437, Some(-141_427)),
        (-141_438, Some(-141_438)),
        (-354_285, Some(-354_285)),
        (-719_162, Some(-719_162)),
        (-719_468, Some(-719_103)),
        (-719_469, None),
        (-719_470, Some(-719_104)),
        (-719_528, Some(-719_162)),
        (-719_529, Some(-718_433)),
        (-719_893, Some(-718_797)),
        (-720_000, Some(-718_174)),
        (-755_994, Some(-682_580)),
        (-1_000_000, Some(-438_622)),
        (-3_000_000, Some(1_561_513)),
    ];

    const JAVA_TIMESTAMP_MILLIS: [(&str, i64); 23] = [
        ("-12219292800.000000000", -12_219_292_800_000),
        ("-12219292800.001000000", -12_219_292_800_001),
        ("-12220156800.000000000", -12_220_156_800_000),
        ("-12220156800.001000000", -12_219_292_800_001),
        ("-12220243200.000000000", -12_219_379_200_000),
        ("-30610224000.000000000", -30_609_792_000_000),
        ("-62135596800.123456789", -62_135_769_600_124),
        ("-62135596800.000000000", -62_135_769_600_000),
        ("-62167219200.000000000", -62_167_392_000_000),
        ("-62198755200.000000000", -62_198_928_000_000),
        ("-100000000000.500000000", -99_999_395_200_500),
        ("-15000000000.250000000", -14_999_222_400_250),
        ("-20000000000.000000000", -19_999_308_800_000),
        ("-40000000000.999999999", -39_999_654_401_000),
        ("-50000000000.000000001", -49_999_913_600_001),
        ("-59000000000.000000000", -59_000_086_400_000),
        ("-62162294400.000000000", -62_162_467_200_000),
        ("-62162380800.000000000", -62_162_553_600_000),
        ("-65318140800.000000000", -65_318_227_200_000),
        ("-86400000000.000000000", -86_399_654_400_000),
        ("-258000000000.000000000", -258_000_406_968_000),
        ("0.000000000", 0),
        ("1700000000.123456789", 1_700_000_000_123),
    ];

    fn column(column_type: SnowflakeColumnType) -> SnowflakeColumn {
        SnowflakeColumn::new("C", column_type, true)
    }

    fn epoch_date(epoch_day: i64) -> NaiveDate {
        let epoch = NaiveDate::default();
        let days = Days::new(epoch_day.unsigned_abs());
        if epoch_day < 0 {
            epoch.checked_sub_days(days).unwrap()
        } else {
            epoch.checked_add_days(days).unwrap()
        }
    }

    fn snowflake_timestamp(raw: &str) -> NaiveDateTime {
        let (negative, digits) = raw
            .strip_prefix('-')
            .map_or((false, raw), |rest| (true, rest));
        let (seconds, fraction) = digits.split_once('.').unwrap();
        let magnitude = i128::from(seconds.parse::<i64>().unwrap()) * 1_000_000_000
            + fraction.parse::<i128>().unwrap();
        let nanos = if negative { -magnitude } else { magnitude };
        let seconds = i64::try_from(nanos.div_euclid(1_000_000_000)).unwrap();
        let nanos = u32::try_from(nanos.rem_euclid(1_000_000_000)).unwrap();
        DateTime::from_timestamp(seconds, nanos)
            .unwrap()
            .naive_utc()
    }

    fn millis(value: &SqlValue) -> i64 {
        value
            .to_timestamp()
            .unwrap()
            .unwrap()
            .and_utc()
            .timestamp_millis()
    }

    #[test]
    fn date_cells_match_the_jdbc_get_date_label() {
        for (epoch_day, expected) in JAVA_DATE_EPOCH_DAYS {
            let date = epoch_date(epoch_day);
            let value =
                decode_cell(&column(SnowflakeColumnType::Date), RawCell::Date(date)).unwrap();
            let actual = value
                .to_date()
                .ok()
                .flatten()
                .map(|d| d.signed_duration_since(NaiveDate::default()).num_days());
            assert_eq!(expected, actual, "epoch day {epoch_day}");
            assert_eq!(
                SqlValue::Date(date).to_java_string(),
                value.to_java_string(),
                "epoch day {epoch_day}"
            );
        }
    }

    #[test]
    fn timestamp_cells_match_the_jdbc_get_timestamp_millis() {
        for (raw, expected) in JAVA_TIMESTAMP_MILLIS {
            let timestamp = snowflake_timestamp(raw);
            let ntz = decode_cell(
                &column(SnowflakeColumnType::TimestampNtz { scale: 9 }),
                RawCell::TimestampNtz(timestamp),
            )
            .unwrap();
            assert_eq!(expected, millis(&ntz), "NTZ {raw}");
            assert_eq!(
                SqlValue::Timestamp(timestamp).to_java_string(),
                ntz.to_java_string(),
                "NTZ {raw}"
            );
            let ltz = decode_cell(
                &column(SnowflakeColumnType::TimestampLtz { scale: 9 }),
                RawCell::TimestampLtz(timestamp.and_utc()),
            )
            .unwrap();
            assert_eq!(expected, millis(&ltz), "LTZ {raw}");
        }
    }

    #[test]
    fn timestamp_tz_cells_match_the_jdbc_get_timestamp_millis() {
        let cases = [
            ("-30610224000.000000000", 0, -30_609_792_000_000),
            ("-30610224000.000000000", 3600, -30_609_792_000_000),
            ("-30610224000.000000000", -9 * 3600, -30_609_792_000_000),
            ("-12220156800.001000000", 3600, -12_219_292_800_001),
            ("-62135596800.123456789", -8 * 3600, -62_135_769_600_124),
        ];
        for (raw, offset_seconds, expected) in cases {
            let offset = FixedOffset::east_opt(offset_seconds).unwrap();
            let timestamp = snowflake_timestamp(raw).and_utc().with_timezone(&offset);
            let value = decode_cell(
                &column(SnowflakeColumnType::TimestampTz { scale: 9 }),
                RawCell::TimestampTz(timestamp),
            )
            .unwrap();
            assert_eq!(expected, millis(&value), "TZ {raw} {offset_seconds}");
            assert_eq!(
                SqlValue::TimestampTz(timestamp).to_java_string(),
                value.to_java_string()
            );
        }
    }
}
