pub mod proto {
    #![allow(clippy::all, clippy::pedantic, missing_docs, rustdoc::all)]
    include!(concat!(env!("OUT_DIR"), "/substrait.rs"));
    pub mod extensions {
        #![allow(clippy::all, clippy::pedantic, missing_docs, rustdoc::all)]
        include!(concat!(env!("OUT_DIR"), "/substrait.extensions.rs"));
    }
}

mod calcite;
mod error;
mod isthmus;
mod model;
mod substrait_accumulator_visitor;
mod substrait_function_parser;
mod substrait_literal_converter;
mod substrait_metadata_parser;
mod substrait_rel_utils;
mod substrait_sql_utils;
mod substrait_type_and_value;
mod util;

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
pub use error::{ErrorKind, Result, SubstraitError};
pub use model::column_predicate::{ColumnPredicate, PredicateValue};
pub use model::logical_expression::LogicalExpression;
pub use model::substrait_field::SubstraitField;
pub use model::substrait_function_names::SubstraitFunctionNames;
pub use model::substrait_operator::SubstraitOperator;
pub use model::substrait_rel_model::SubstraitRelModel;
pub use substrait_accumulator_visitor::SubstraitAccumulatorVisitor;
pub use substrait_function_parser::SubstraitFunctionParser;
pub use substrait_literal_converter::{LiteralValue, SubstraitLiteralConverter};
pub use substrait_metadata_parser::SubstraitMetadataParser;
pub use substrait_rel_utils::{SubstraitRelUtils, PLAN_PARSER_RECURSION_LIMIT};
pub use substrait_sql_utils::SubstraitSqlUtils;
pub use substrait_type_and_value::{
    SubstraitTypeAndValue, SubstraitTypeAndValueBuilder, SubstraitValue,
};
pub use util::limit_and_sort_helper::{GenericSortField, LimitAndSortHelper};
