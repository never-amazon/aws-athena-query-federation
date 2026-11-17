#[allow(clippy::cast_possible_truncation, reason = "matches Java narrowing")]
pub(crate) fn long_to_int(v: i64) -> i32 {
    v as i32
}

#[allow(clippy::cast_possible_truncation, reason = "matches Java narrowing")]
pub(crate) fn long_to_short(v: i64) -> i16 {
    v as i16
}

#[allow(clippy::cast_possible_truncation, reason = "matches Java narrowing")]
pub(crate) fn long_to_byte(v: i64) -> i8 {
    v as i8
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "matches Java narrowing"
)]
pub(crate) fn long_to_char(v: i64) -> u16 {
    v as u16
}

#[allow(clippy::cast_precision_loss, reason = "matches Java widening")]
pub(crate) fn long_to_double(v: i64) -> f64 {
    v as f64
}

#[allow(clippy::cast_precision_loss, reason = "matches Java widening")]
pub(crate) fn long_to_float(v: i64) -> f32 {
    v as f32
}

#[allow(clippy::cast_possible_truncation, reason = "matches Java narrowing")]
pub(crate) fn double_to_float(v: f64) -> f32 {
    v as f32
}

#[allow(clippy::cast_possible_truncation, reason = "matches Java narrowing")]
pub(crate) fn double_to_long(v: f64) -> i64 {
    v as i64
}

#[allow(clippy::cast_possible_truncation, reason = "matches Java narrowing")]
pub(crate) fn double_to_int(v: f64) -> i32 {
    v as i32
}

#[allow(clippy::cast_precision_loss, reason = "matches Java widening")]
pub(crate) fn int_to_float(v: i32) -> f32 {
    v as f32
}

pub(crate) fn len_to_long(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}
