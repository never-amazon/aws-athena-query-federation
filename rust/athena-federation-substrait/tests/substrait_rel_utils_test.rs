use athena_federation_substrait::proto::expression::RexType;
use athena_federation_substrait::proto::fetch_rel::CountMode;
use athena_federation_substrait::proto::function_argument::ArgType;
use athena_federation_substrait::proto::plan_rel;
use athena_federation_substrait::proto::rel::RelType;
use athena_federation_substrait::proto::{
    expression, Expression, FetchRel, FilterRel, FunctionArgument, Plan, PlanRel, ProjectRel,
    ReadRel, Rel, RelRoot, SortRel,
};
use athena_federation_substrait::SubstraitRelUtils;
use base64::Engine;
use prost::Message;

fn rel(rel_type: RelType) -> Rel {
    Rel {
        rel_type: Some(rel_type),
    }
}

fn encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[test]
fn test_get_filter_rel_found() {
    let expected = FilterRel::default();
    let r = rel(RelType::Filter(Box::new(expected.clone())));
    assert_eq!(Some(&expected), SubstraitRelUtils::get_filter_rel(Some(&r)));
}

#[test]
fn test_get_filter_rel_not_found() {
    let r = rel(RelType::Read(Box::default()));
    assert!(SubstraitRelUtils::get_filter_rel(Some(&r)).is_none());
}

#[test]
fn test_get_filter_rel_with_nested_structure() {
    let expected = FilterRel::default();
    let inner = rel(RelType::Filter(Box::new(expected.clone())));
    let outer = rel(RelType::Project(Box::new(ProjectRel {
        input: Some(Box::new(inner)),
        ..Default::default()
    })));
    assert_eq!(
        Some(&expected),
        SubstraitRelUtils::get_filter_rel(Some(&outer))
    );
}

#[test]
#[allow(deprecated)]
fn test_get_fetch_rel_found() {
    let expected = FetchRel {
        count_mode: Some(CountMode::Count(10)),
        ..Default::default()
    };
    let r = rel(RelType::Fetch(Box::new(expected.clone())));
    assert_eq!(Some(&expected), SubstraitRelUtils::get_fetch_rel(Some(&r)));
}

#[test]
fn test_get_fetch_rel_not_found() {
    let r = rel(RelType::Read(Box::default()));
    assert!(SubstraitRelUtils::get_fetch_rel(Some(&r)).is_none());
}

#[test]
fn test_get_sort_rel_found() {
    let expected = SortRel::default();
    let r = rel(RelType::Sort(Box::new(expected.clone())));
    assert_eq!(Some(&expected), SubstraitRelUtils::get_sort_rel(Some(&r)));
}

#[test]
fn test_get_sort_rel_not_found() {
    let r = rel(RelType::Read(Box::default()));
    assert!(SubstraitRelUtils::get_sort_rel(Some(&r)).is_none());
}

#[test]
fn test_get_read_rel_found() {
    let expected = ReadRel::default();
    let r = rel(RelType::Read(Box::new(expected.clone())));
    assert_eq!(Some(&expected), SubstraitRelUtils::get_read_rel(Some(&r)));
}

#[test]
fn test_get_read_rel_not_found() {
    let r = rel(RelType::Filter(Box::default()));
    assert!(SubstraitRelUtils::get_read_rel(Some(&r)).is_none());
}

#[test]
fn test_get_project_rel_found() {
    let expected = ProjectRel::default();
    let r = rel(RelType::Project(Box::new(expected.clone())));
    assert_eq!(
        Some(&expected),
        SubstraitRelUtils::get_project_rel(Some(&r))
    );
}

#[test]
fn test_get_project_rel_not_found() {
    let r = rel(RelType::Read(Box::default()));
    assert!(SubstraitRelUtils::get_project_rel(Some(&r)).is_none());
}

#[test]
fn test_deserialize_substrait_plan_valid() {
    let original = Plan::default();
    let encoded = encode(&original.encode_to_vec());
    let result = SubstraitRelUtils::deserialize_substrait_plan(&encoded).expect("valid plan");
    assert_eq!(original, result);
}

#[test]
fn test_deserialize_substrait_plan_invalid_base64() {
    let err =
        SubstraitRelUtils::deserialize_substrait_plan("invalid-base64-string!@#").unwrap_err();
    assert_eq!("Failed to parse Substrait plan", err.message());
}

#[test]
fn test_deserialize_substrait_plan_invalid_protobuf() {
    let encoded = encode(b"invalid protobuf data");
    let err = SubstraitRelUtils::deserialize_substrait_plan(&encoded).unwrap_err();
    assert_eq!("Failed to parse Substrait plan", err.message());
}

#[test]
fn test_traversal_with_null_rel() {
    assert!(SubstraitRelUtils::get_filter_rel(None).is_none());
}

#[test]
fn test_complex_nested_traversal() {
    let expected = ReadRel::default();
    let read = rel(RelType::Read(Box::new(expected.clone())));
    let project = rel(RelType::Project(Box::new(ProjectRel {
        input: Some(Box::new(read)),
        ..Default::default()
    })));
    let filter = rel(RelType::Filter(Box::new(FilterRel {
        input: Some(Box::new(project)),
        ..Default::default()
    })));
    let sort = rel(RelType::Sort(Box::new(SortRel {
        input: Some(Box::new(filter)),
        ..Default::default()
    })));
    let fetch = rel(RelType::Fetch(Box::new(FetchRel {
        input: Some(Box::new(sort)),
        ..Default::default()
    })));
    assert_eq!(
        Some(&expected),
        SubstraitRelUtils::get_read_rel(Some(&fetch))
    );
}

fn nested_condition(levels: usize) -> Expression {
    let mut condition = Expression::default();
    for _ in 0..levels {
        condition = Expression {
            rex_type: Some(RexType::ScalarFunction(expression::ScalarFunction {
                arguments: vec![FunctionArgument {
                    arg_type: Some(ArgType::Value(condition)),
                }],
                ..Default::default()
            })),
        };
    }
    condition
}

fn plan_with_condition(condition: Expression) -> Plan {
    Plan {
        relations: vec![PlanRel {
            rel_type: Some(plan_rel::RelType::Root(RelRoot {
                input: Some(rel(RelType::Filter(Box::new(FilterRel {
                    condition: Some(Box::new(condition)),
                    ..Default::default()
                })))),
                names: vec![],
            })),
        }],
        ..Default::default()
    }
}

#[test]
fn test_deserialize_substrait_plan_with_deep_nesting() {
    let plan = plan_with_condition(nested_condition(200));
    let encoded = encode(&plan.encode_to_vec());
    let result = SubstraitRelUtils::deserialize_substrait_plan(&encoded).expect("deep plan parses");
    assert_eq!(plan, result);
}

#[test]
fn deserialize_substrait_plan_rejects_nesting_past_java_limit() {
    let handle = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let plan = plan_with_condition(nested_condition(700));
            let encoded = encode(&plan.encode_to_vec());
            let err = SubstraitRelUtils::deserialize_substrait_plan(&encoded).unwrap_err();
            assert_eq!("Failed to parse Substrait plan", err.message());
            assert!(err
                .root_cause()
                .message()
                .contains("too many levels of nesting"));
        })
        .expect("spawn");
    handle.join().expect("thread");
}

#[test]
fn deserialize_substrait_plan_accepts_missing_padding() {
    let plan = Plan {
        expected_type_urls: vec!["ab".to_string()],
        ..Default::default()
    };
    let encoded = encode(&plan.encode_to_vec());
    let trimmed = encoded.trim_end_matches('=');
    assert_ne!(encoded, trimmed);
    assert_eq!(
        plan,
        SubstraitRelUtils::deserialize_substrait_plan(trimmed).expect("unpadded")
    );
}
