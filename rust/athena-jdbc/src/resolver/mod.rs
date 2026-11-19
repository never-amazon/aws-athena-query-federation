mod jdbc_case_resolver;

pub use jdbc_case_resolver::{
    table_name_from_query_annotation, AnnotationFallback, CaseLookup, JdbcCaseResolver,
    ANNOTATION_SUPPORTED_SOURCE_TYPES,
};
