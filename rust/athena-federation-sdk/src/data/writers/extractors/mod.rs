use std::any::Any;
use std::fmt;
use std::sync::Arc;

use super::holders::*;
use crate::exceptions::Result;

pub trait BigIntExtractor: Send + Sync {
    fn extract(&self, context: &dyn Any, dst: &mut NullableBigIntHolder) -> Result<()>;
}

impl<F> BigIntExtractor for F
where
    F: Fn(&dyn Any, &mut NullableBigIntHolder) -> Result<()> + Send + Sync,
{
    fn extract(&self, context: &dyn Any, dst: &mut NullableBigIntHolder) -> Result<()> {
        self(context, dst)
    }
}

pub trait BitExtractor: Send + Sync {
    fn extract(&self, context: &dyn Any, dst: &mut NullableBitHolder) -> Result<()>;
}

impl<F> BitExtractor for F
where
    F: Fn(&dyn Any, &mut NullableBitHolder) -> Result<()> + Send + Sync,
{
    fn extract(&self, context: &dyn Any, dst: &mut NullableBitHolder) -> Result<()> {
        self(context, dst)
    }
}

pub trait DateDayExtractor: Send + Sync {
    fn extract(&self, context: &dyn Any, dst: &mut NullableDateDayHolder) -> Result<()>;
}

impl<F> DateDayExtractor for F
where
    F: Fn(&dyn Any, &mut NullableDateDayHolder) -> Result<()> + Send + Sync,
{
    fn extract(&self, context: &dyn Any, dst: &mut NullableDateDayHolder) -> Result<()> {
        self(context, dst)
    }
}

pub trait DateMilliExtractor: Send + Sync {
    fn extract(&self, context: &dyn Any, dst: &mut NullableDateMilliHolder) -> Result<()>;
}

impl<F> DateMilliExtractor for F
where
    F: Fn(&dyn Any, &mut NullableDateMilliHolder) -> Result<()> + Send + Sync,
{
    fn extract(&self, context: &dyn Any, dst: &mut NullableDateMilliHolder) -> Result<()> {
        self(context, dst)
    }
}

pub trait DecimalExtractor: Send + Sync {
    fn extract(&self, context: &dyn Any, dst: &mut NullableDecimalHolder) -> Result<()>;
}

impl<F> DecimalExtractor for F
where
    F: Fn(&dyn Any, &mut NullableDecimalHolder) -> Result<()> + Send + Sync,
{
    fn extract(&self, context: &dyn Any, dst: &mut NullableDecimalHolder) -> Result<()> {
        self(context, dst)
    }
}

pub trait Float4Extractor: Send + Sync {
    fn extract(&self, context: &dyn Any, dst: &mut NullableFloat4Holder) -> Result<()>;
}

impl<F> Float4Extractor for F
where
    F: Fn(&dyn Any, &mut NullableFloat4Holder) -> Result<()> + Send + Sync,
{
    fn extract(&self, context: &dyn Any, dst: &mut NullableFloat4Holder) -> Result<()> {
        self(context, dst)
    }
}

pub trait Float8Extractor: Send + Sync {
    fn extract(&self, context: &dyn Any, dst: &mut NullableFloat8Holder) -> Result<()>;
}

impl<F> Float8Extractor for F
where
    F: Fn(&dyn Any, &mut NullableFloat8Holder) -> Result<()> + Send + Sync,
{
    fn extract(&self, context: &dyn Any, dst: &mut NullableFloat8Holder) -> Result<()> {
        self(context, dst)
    }
}

pub trait IntExtractor: Send + Sync {
    fn extract(&self, context: &dyn Any, dst: &mut NullableIntHolder) -> Result<()>;
}

impl<F> IntExtractor for F
where
    F: Fn(&dyn Any, &mut NullableIntHolder) -> Result<()> + Send + Sync,
{
    fn extract(&self, context: &dyn Any, dst: &mut NullableIntHolder) -> Result<()> {
        self(context, dst)
    }
}

pub trait SmallIntExtractor: Send + Sync {
    fn extract(&self, context: &dyn Any, dst: &mut NullableSmallIntHolder) -> Result<()>;
}

impl<F> SmallIntExtractor for F
where
    F: Fn(&dyn Any, &mut NullableSmallIntHolder) -> Result<()> + Send + Sync,
{
    fn extract(&self, context: &dyn Any, dst: &mut NullableSmallIntHolder) -> Result<()> {
        self(context, dst)
    }
}

pub trait TinyIntExtractor: Send + Sync {
    fn extract(&self, context: &dyn Any, dst: &mut NullableTinyIntHolder) -> Result<()>;
}

impl<F> TinyIntExtractor for F
where
    F: Fn(&dyn Any, &mut NullableTinyIntHolder) -> Result<()> + Send + Sync,
{
    fn extract(&self, context: &dyn Any, dst: &mut NullableTinyIntHolder) -> Result<()> {
        self(context, dst)
    }
}

pub trait VarBinaryExtractor: Send + Sync {
    fn extract(&self, context: &dyn Any, dst: &mut NullableVarBinaryHolder) -> Result<()>;
}

impl<F> VarBinaryExtractor for F
where
    F: Fn(&dyn Any, &mut NullableVarBinaryHolder) -> Result<()> + Send + Sync,
{
    fn extract(&self, context: &dyn Any, dst: &mut NullableVarBinaryHolder) -> Result<()> {
        self(context, dst)
    }
}

pub trait VarCharExtractor: Send + Sync {
    fn extract(&self, context: &dyn Any, dst: &mut NullableVarCharHolder) -> Result<()>;
}

impl<F> VarCharExtractor for F
where
    F: Fn(&dyn Any, &mut NullableVarCharHolder) -> Result<()> + Send + Sync,
{
    fn extract(&self, context: &dyn Any, dst: &mut NullableVarCharHolder) -> Result<()> {
        self(context, dst)
    }
}

#[derive(Clone)]
pub enum Extractor {
    BigInt(Arc<dyn BigIntExtractor>),
    Bit(Arc<dyn BitExtractor>),
    DateDay(Arc<dyn DateDayExtractor>),
    DateMilli(Arc<dyn DateMilliExtractor>),
    Decimal(Arc<dyn DecimalExtractor>),
    Float4(Arc<dyn Float4Extractor>),
    Float8(Arc<dyn Float8Extractor>),
    Int(Arc<dyn IntExtractor>),
    SmallInt(Arc<dyn SmallIntExtractor>),
    TinyInt(Arc<dyn TinyIntExtractor>),
    VarBinary(Arc<dyn VarBinaryExtractor>),
    VarChar(Arc<dyn VarCharExtractor>),
}

impl Extractor {
    pub fn type_name(&self) -> &'static str {
        match self {
            Extractor::BigInt(_) => "BigIntExtractor",
            Extractor::Bit(_) => "BitExtractor",
            Extractor::DateDay(_) => "DateDayExtractor",
            Extractor::DateMilli(_) => "DateMilliExtractor",
            Extractor::Decimal(_) => "DecimalExtractor",
            Extractor::Float4(_) => "Float4Extractor",
            Extractor::Float8(_) => "Float8Extractor",
            Extractor::Int(_) => "IntExtractor",
            Extractor::SmallInt(_) => "SmallIntExtractor",
            Extractor::TinyInt(_) => "TinyIntExtractor",
            Extractor::VarBinary(_) => "VarBinaryExtractor",
            Extractor::VarChar(_) => "VarCharExtractor",
        }
    }

    pub fn big_int<F>(f: F) -> Extractor
    where
        F: Fn(&dyn Any, &mut NullableBigIntHolder) -> Result<()> + Send + Sync + 'static,
    {
        Extractor::BigInt(Arc::new(f))
    }

    pub fn bit<F>(f: F) -> Extractor
    where
        F: Fn(&dyn Any, &mut NullableBitHolder) -> Result<()> + Send + Sync + 'static,
    {
        Extractor::Bit(Arc::new(f))
    }

    pub fn date_day<F>(f: F) -> Extractor
    where
        F: Fn(&dyn Any, &mut NullableDateDayHolder) -> Result<()> + Send + Sync + 'static,
    {
        Extractor::DateDay(Arc::new(f))
    }

    pub fn date_milli<F>(f: F) -> Extractor
    where
        F: Fn(&dyn Any, &mut NullableDateMilliHolder) -> Result<()> + Send + Sync + 'static,
    {
        Extractor::DateMilli(Arc::new(f))
    }

    pub fn decimal<F>(f: F) -> Extractor
    where
        F: Fn(&dyn Any, &mut NullableDecimalHolder) -> Result<()> + Send + Sync + 'static,
    {
        Extractor::Decimal(Arc::new(f))
    }

    pub fn float4<F>(f: F) -> Extractor
    where
        F: Fn(&dyn Any, &mut NullableFloat4Holder) -> Result<()> + Send + Sync + 'static,
    {
        Extractor::Float4(Arc::new(f))
    }

    pub fn float8<F>(f: F) -> Extractor
    where
        F: Fn(&dyn Any, &mut NullableFloat8Holder) -> Result<()> + Send + Sync + 'static,
    {
        Extractor::Float8(Arc::new(f))
    }

    pub fn int<F>(f: F) -> Extractor
    where
        F: Fn(&dyn Any, &mut NullableIntHolder) -> Result<()> + Send + Sync + 'static,
    {
        Extractor::Int(Arc::new(f))
    }

    pub fn small_int<F>(f: F) -> Extractor
    where
        F: Fn(&dyn Any, &mut NullableSmallIntHolder) -> Result<()> + Send + Sync + 'static,
    {
        Extractor::SmallInt(Arc::new(f))
    }

    pub fn tiny_int<F>(f: F) -> Extractor
    where
        F: Fn(&dyn Any, &mut NullableTinyIntHolder) -> Result<()> + Send + Sync + 'static,
    {
        Extractor::TinyInt(Arc::new(f))
    }

    pub fn var_binary<F>(f: F) -> Extractor
    where
        F: Fn(&dyn Any, &mut NullableVarBinaryHolder) -> Result<()> + Send + Sync + 'static,
    {
        Extractor::VarBinary(Arc::new(f))
    }

    pub fn var_char<F>(f: F) -> Extractor
    where
        F: Fn(&dyn Any, &mut NullableVarCharHolder) -> Result<()> + Send + Sync + 'static,
    {
        Extractor::VarChar(Arc::new(f))
    }
}

impl fmt::Debug for Extractor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.type_name())
    }
}
