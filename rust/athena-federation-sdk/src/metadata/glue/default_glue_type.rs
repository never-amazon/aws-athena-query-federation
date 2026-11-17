use std::sync::LazyLock;

use arrow::datatypes::{DataType, TimeUnit};
use regex::Regex;

use crate::data::{timestamp_tz, ArrowType};
use crate::exceptions::{FederationError, Result};

const TIMESTAMPMILLITZ: &str = "timestamptz";
const NON_COMPARABLE_SET: [&str; 1] = ["TIMESTAMPMILLITZ"];
const DEFAULT_DECIMAL_PRECISION: u8 = 38;
const DEFAULT_DECIMAL_SCALE: i8 = 18;
const DEFAULT_DECIMAL_BIT_WIDTH: i32 = 128;

static DECIMAL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^decimal([(](([0-9]+,?){2,3})[)])?$").expect("valid decimal pattern")
});

static SYSTEM_ZONE_ID: LazyLock<String> = LazyLock::new(system_default_zone_id);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultGlueType {
    id: String,
    arrow_type: ArrowType,
}

impl DefaultGlueType {
    pub fn new(id: impl Into<String>, arrow_type: ArrowType) -> DefaultGlueType {
        DefaultGlueType {
            id: id.into(),
            arrow_type,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn arrow_type(&self) -> &ArrowType {
        &self.arrow_type
    }

    pub fn from_id(id: &str) -> Result<ArrowType> {
        DefaultGlueType::to_arrow_type(id)?.ok_or_else(|| {
            FederationError::invalid_input(format!("Unknown DefaultGlueType for id: {id}"))
        })
    }

    pub fn to_arrow_type(id: &str) -> Result<Option<ArrowType>> {
        match base_type(&id.to_lowercase()) {
            Some(arrow_type) => Ok(Some(arrow_type)),
            None => decimal_arrow_type(id),
        }
    }

    pub fn non_comparable_set() -> &'static [&'static str] {
        &NON_COMPARABLE_SET
    }
}

fn base_type(id: &str) -> Option<ArrowType> {
    Some(match id {
        "int" => DataType::Int32,
        "varchar" | "string" => DataType::Utf8,
        "bigint" => DataType::Int64,
        "double" => DataType::Float64,
        "float" => DataType::Float32,
        "smallint" => DataType::Int16,
        "tinyint" => DataType::Int8,
        "boolean" => DataType::Boolean,
        "binary" => DataType::Binary,
        "timestamp" => DataType::Date64,
        "date" => DataType::Date32,
        TIMESTAMPMILLITZ => timestamp_tz(TimeUnit::Millisecond, &SYSTEM_ZONE_ID),
        _ => return None,
    })
}

fn decimal_arrow_type(id: &str) -> Result<Option<ArrowType>> {
    let Some(captures) = DECIMAL_PATTERN.captures(id) else {
        return Ok(None);
    };
    let Some(params) = captures.get(2) else {
        return Ok(Some(DataType::Decimal128(
            DEFAULT_DECIMAL_PRECISION,
            DEFAULT_DECIMAL_SCALE,
        )));
    };
    let params = params
        .as_str()
        .split(',')
        .filter(|p| !p.is_empty())
        .map(parse_int)
        .collect::<Result<Vec<i32>>>()?;
    let (precision, scale, bit_width) = match params.as_slice() {
        [precision, scale] => (*precision, *scale, DEFAULT_DECIMAL_BIT_WIDTH),
        [precision, scale, bit_width] => (*precision, *scale, *bit_width),
        _ => return Ok(None),
    };
    let unsupported = || {
        FederationError::invalid_input(format!(
            "Unsupported decimal type {id}: arrow-rs supports precision up to 255, scale up to 127 and bit widths 32, 64, 128 and 256"
        ))
    };
    let precision = u8::try_from(precision).map_err(|_| unsupported())?;
    let scale = i8::try_from(scale).map_err(|_| unsupported())?;
    Ok(Some(match bit_width {
        32 => DataType::Decimal32(precision, scale),
        64 => DataType::Decimal64(precision, scale),
        128 => DataType::Decimal128(precision, scale),
        256 => DataType::Decimal256(precision, scale),
        _ => return Err(unsupported()),
    }))
}

fn parse_int(value: &str) -> Result<i32> {
    value
        .parse::<i32>()
        .map_err(|_| FederationError::NumberFormat(format!("For input string: \"{value}\"")))
}

fn system_default_zone_id() -> String {
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim_start_matches(':');
        if !tz.is_empty() {
            return tz.to_string();
        }
    }
    std::fs::read_link("/etc/localtime")
        .ok()
        .and_then(|target| {
            let target = target.to_string_lossy().into_owned();
            target
                .split_once("zoneinfo/")
                .map(|(_, zone)| zone.to_string())
        })
        .unwrap_or_else(|| "UTC".to_string())
}
