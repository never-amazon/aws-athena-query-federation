pub mod proto {
    #![allow(
        clippy::all,
        clippy::pedantic,
        missing_docs,
        rustdoc::all,
        reason = "generated prost code"
    )]
    include!(concat!(env!("OUT_DIR"), "/substrait.rs"));
    pub mod extensions {
        #![allow(
            clippy::all,
            clippy::pedantic,
            missing_docs,
            rustdoc::all,
            reason = "generated prost code"
        )]
        include!(concat!(env!("OUT_DIR"), "/substrait.extensions.rs"));
    }
}

mod calcite;
mod error;
mod isthmus;
mod java_cast;
pub mod literal;
mod params;
pub mod plan;
pub mod predicate;
mod substrait_type_and_value;

pub use calcite::operators;
pub use calcite::operators::{SqlKind, SqlOperator, SqlSyntax};
pub use calcite::rel_data_type::{
    RelDataType, RelDataTypeBuilder, RelDataTypeField, RelDataTypeSystem, PRECISION_NOT_SPECIFIED,
    SCALE_NOT_SPECIFIED,
};
pub use calcite::sql_dialect::{Conformance, DatabaseProduct, SqlDialect};
pub use calcite::sql_node::{
    LiteralObject, SqlCall, SqlCase, SqlDataTypeSpec, SqlIdentifier, SqlLiteral, SqlLiteralValue,
    SqlNode, SqlSelect, SqlWindow,
};
pub use calcite::sql_type_name::SqlTypeName;
pub use calcite::values::{BitString, DateString, TimeString, TimestampString};
pub use error::{Result, SubstraitError};
pub use literal::LiteralValue;
pub use params::parameterize;
pub use plan::PLAN_PARSER_RECURSION_LIMIT;
pub use predicate::{ColumnPredicate, LogicalExpression, SubstraitOperator};
pub use substrait_type_and_value::{
    SubstraitTypeAndValue, SubstraitTypeAndValueBuilder, SubstraitValue,
};
