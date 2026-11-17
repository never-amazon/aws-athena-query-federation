use bigdecimal::BigDecimal;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullableBigIntHolder {
    pub is_set: i32,
    pub value: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullableBitHolder {
    pub is_set: i32,
    pub value: i32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullableDateDayHolder {
    pub is_set: i32,
    pub value: i32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullableDateMilliHolder {
    pub is_set: i32,
    pub value: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullableDecimalHolder {
    pub is_set: i32,
    pub value: BigDecimal,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullableFloat4Holder {
    pub is_set: i32,
    pub value: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullableFloat8Holder {
    pub is_set: i32,
    pub value: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullableIntHolder {
    pub is_set: i32,
    pub value: i32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullableSmallIntHolder {
    pub is_set: i32,
    pub value: i16,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullableTinyIntHolder {
    pub is_set: i32,
    pub value: i8,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullableVarBinaryHolder {
    pub is_set: i32,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullableVarCharHolder {
    pub is_set: i32,
    pub value: String,
}
