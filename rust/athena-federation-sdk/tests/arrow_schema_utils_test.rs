use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Fields, Metadata};
use athena_federation_sdk::data::get_children;
use athena_federation_sdk::data::ArrowSchemaUtils;

fn meta(k: &str, v: &str) -> Metadata {
    Metadata::from(HashMap::from([(k.to_string(), v.to_string())]))
}

fn render(field: &Field) -> String {
    let children = get_children(field);
    let type_name = match field.data_type() {
        DataType::Struct(_) => "Struct".to_string(),
        DataType::Decimal32(p, s) => format!("Decimal({p}, {s}, 32)"),
        other => format!("{other:?}"),
    };
    if children.is_empty() {
        format!("{}: {}", field.name(), type_name)
    } else {
        let inner: Vec<String> = children.iter().map(|c| render(c)).collect();
        format!("{}: {}<{}>", field.name(), type_name, inner.join(", "))
    }
}

#[test]
fn test_remap_type_in_struct() {
    let bsdf_meta = meta("11111111", "2222222222");
    let bsdf = Field::new("bsdf", DataType::Utf8, true).with_metadata(bsdf_meta.clone());
    let csdf_meta = meta("3333333333", "4444444444");
    let csdf =
        Field::new("csdf", DataType::Decimal32(12, 3), true).with_metadata(csdf_meta.clone());
    let asdf_meta = meta("55555555", "66666666");
    let asdf = Field::new(
        "asdf",
        DataType::Struct(Fields::from(vec![bsdf.clone(), csdf.clone()])),
        true,
    )
    .with_metadata(asdf_meta.clone());
    let outer_meta = meta("7777", "88888");
    let outer = Field::new(
        "aasdf",
        DataType::Struct(Fields::from(vec![asdf, bsdf, csdf])),
        true,
    )
    .with_metadata(outer_meta.clone());

    let mapper = |arrow_type: &DataType| -> DataType {
        if *arrow_type == DataType::Utf8 {
            DataType::Decimal32(24, 6)
        } else {
            arrow_type.clone()
        }
    };
    let output_field = ArrowSchemaUtils::remap_arrow_types_within_field(&outer, &mapper);
    assert_eq!(
        "aasdf: Struct<asdf: Struct<bsdf: Decimal(24, 6, 32), csdf: Decimal(12, 3, 32)>, bsdf: Decimal(24, 6, 32), csdf: Decimal(12, 3, 32)>",
        render(&output_field)
    );
    assert!(output_field.is_nullable());
    assert_eq!(&outer_meta, output_field.metadata());
    let children = get_children(&output_field);
    let output_asdf = Arc::clone(&children[0]);
    assert!(output_asdf.is_nullable());
    assert_eq!(&asdf_meta, output_asdf.metadata());
    let inner = get_children(&output_asdf);
    assert!(inner[0].is_nullable());
    assert_eq!(&bsdf_meta, inner[0].metadata());
    assert!(inner[1].is_nullable());
    assert_eq!(&csdf_meta, inner[1].metadata());
    assert!(children[1].is_nullable());
    assert_eq!(&bsdf_meta, children[1].metadata());
    assert!(children[2].is_nullable());
    assert_eq!(&csdf_meta, children[2].metadata());
}
