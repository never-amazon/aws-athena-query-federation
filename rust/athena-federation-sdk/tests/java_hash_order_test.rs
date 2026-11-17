use athena_federation_sdk::data::java_format::java_string_hash;
use athena_federation_sdk::serde::java_hash::{
    java_hash_map_order, java_hash_order_default, java_hash_order_with_capacity,
    java_hash_set_copy_order, java_hash_set_order_by_hash,
    java_hash_set_order_by_hash_with_compare,
};

const COLLIDING: [&str; 10] = [
    "AaAaAaAaAa",
    "AaAaAaAaBB",
    "AaAaAaBBAa",
    "AaAaAaBBBB",
    "AaAaBBAaAa",
    "AaAaBBAaBB",
    "AaAaBBBBAa",
    "AaAaBBBBBB",
    "AaBBAaAaAa",
    "AaBBAaAaBB",
];

fn forty_then_seven_colliding() -> Vec<String> {
    let mut keys: Vec<String> = (0..40).map(|i| format!("x{i}")).collect();
    keys.extend(COLLIDING[..7].iter().map(|k| (*k).to_string()));
    keys
}

fn colliding_then_five(colliding: usize) -> Vec<String> {
    let mut keys: Vec<String> = COLLIDING[..colliding]
        .iter()
        .map(|k| (*k).to_string())
        .collect();
    keys.extend((0..5).map(|i| format!("x{i}")));
    keys
}

const JAVA_FORTY_SEVEN: &str = "x8,x9,x30,x10,x32,x31,x12,x34,x11,x33,x14,x36,x13,x35,x16,x38,\
AaAaAaAaBB,x15,x37,AaAaAaAaAa,AaAaAaBBAa,AaAaAaBBBB,AaAaBBAaAa,AaAaBBAaBB,AaAaBBBBAa,x18,x17,x39,\
x19,x21,x20,x0,x23,x1,x22,x2,x25,x3,x24,x4,x27,x5,x26,x6,x29,x7,x28";

const JAVA_TREE_BIN_FIFTEEN: &str = "AaAaAaAaAa,AaAaAaAaBB,AaAaAaBBAa,AaAaAaBBBB,AaAaBBAaAa,\
AaAaBBAaBB,AaAaBBBBAa,AaAaBBBBBB,AaBBAaAaAa,AaBBAaAaBB,x0,x1,x2,x3,x4";

#[test]
fn default_order_matches_java_after_tree_bin_splits_on_resize() {
    let keys = forty_then_seven_colliding();
    let order = java_hash_order_default(keys.iter().map(String::as_str));
    assert_eq!(JAVA_FORTY_SEVEN, order.join(","));
}

#[test]
fn set_copy_order_matches_java_after_tree_bin_splits_on_resize() {
    let keys = forty_then_seven_colliding();
    let order = java_hash_set_copy_order(keys.iter().map(String::as_str));
    assert_eq!(JAVA_FORTY_SEVEN, order.join(","));
}

#[test]
fn capacity_order_matches_java_when_root_moves_to_front() {
    let keys = colliding_then_five(9);
    let order = java_hash_order_with_capacity(keys.iter().map(String::as_str), 64);
    assert_eq!(
        "AaAaAaBBBB,AaAaAaAaAa,AaAaAaAaBB,AaAaAaBBAa,AaAaBBAaAa,AaAaBBAaBB,AaAaBBBBAa,\
AaAaBBBBBB,AaBBAaAaAa,x0,x1,x2,x3,x4",
        order.join(",")
    );
}

#[test]
fn hash_map_order_matches_java_for_tree_bin_after_resizes() {
    let keys = colliding_then_five(10);
    let order: Vec<&str> = java_hash_map_order(keys.iter())
        .into_iter()
        .map(String::as_str)
        .collect();
    assert_eq!(JAVA_TREE_BIN_FIFTEEN, order.join(","));
}

#[test]
fn hash_set_order_by_hash_with_compare_matches_java_for_tree_bin() {
    let keys = colliding_then_five(10);
    let order = java_hash_set_order_by_hash_with_compare(
        keys.iter().map(String::as_str).collect(),
        |k| java_string_hash(k),
        |a, b| a.encode_utf16().cmp(b.encode_utf16()),
    );
    assert_eq!(JAVA_TREE_BIN_FIFTEEN, order.join(","));
}

#[test]
fn hash_set_order_by_hash_keeps_bucket_order_without_collisions() {
    let keys: Vec<String> = (0..12).map(|i| format!("col{i}")).collect();
    let order = java_hash_set_order_by_hash(keys.iter().map(String::as_str).collect(), |k| {
        java_string_hash(k)
    });
    assert_eq!(
        "col8,col9,col11,col6,col10,col7,col4,col5,col2,col3,col0,col1",
        order.join(",")
    );
}
