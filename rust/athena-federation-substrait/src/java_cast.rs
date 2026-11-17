#[allow(clippy::cast_possible_truncation, reason = "matches Java narrowing")]
pub(crate) fn long_to_int(v: i64) -> i32 {
    v as i32
}

#[allow(clippy::cast_possible_truncation, reason = "matches Java narrowing")]
pub(crate) fn int_to_byte(v: i32) -> i8 {
    v as i8
}

#[allow(clippy::cast_possible_truncation, reason = "matches Java narrowing")]
pub(crate) fn int_to_short(v: i32) -> i16 {
    v as i16
}

#[allow(clippy::cast_possible_truncation, reason = "matches Java narrowing")]
pub(crate) fn double_to_long(v: f64) -> i64 {
    v as i64
}

#[allow(clippy::cast_precision_loss, reason = "matches Java widening")]
pub(crate) fn long_to_double(v: i64) -> f64 {
    v as f64
}
