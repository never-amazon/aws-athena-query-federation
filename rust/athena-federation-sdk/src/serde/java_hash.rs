use crate::data::java_format::java_string_hash;

const DEFAULT_INITIAL_CAPACITY: usize = 16;
const MAXIMUM_CAPACITY: usize = 1 << 30;

fn spread(key: &str) -> u32 {
    let h = java_string_hash(key) as u32;
    h ^ (h >> 16)
}

fn table_size_for(cap: usize) -> usize {
    if cap <= 1 {
        return 1;
    }
    let n = (cap - 1).next_power_of_two();
    n.clamp(1, MAXIMUM_CAPACITY)
}

fn final_table_size(initial_table: usize, distinct: usize) -> usize {
    let mut table = initial_table;
    let mut threshold = (table as f64 * 0.75) as usize;
    for size in 1..=distinct {
        if size > threshold && table < MAXIMUM_CAPACITY {
            table *= 2;
            threshold = (table as f64 * 0.75) as usize;
        }
    }
    table
}

pub fn java_hash_order_with_table<'a, I>(keys_in_insertion_order: I, initial_table: usize) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut distinct: Vec<&'a str> = Vec::new();
    for k in keys_in_insertion_order {
        if !distinct.contains(&k) {
            distinct.push(k);
        }
    }
    let table = final_table_size(initial_table, distinct.len());
    let mut indexed: Vec<(usize, usize, &'a str)> = distinct
        .iter()
        .enumerate()
        .map(|(i, k)| ((spread(k) as usize) & (table - 1), i, *k))
        .collect();
    indexed.sort_by_key(|(bucket, order, _)| (*bucket, *order));
    indexed.into_iter().map(|(_, _, k)| k).collect()
}

pub fn java_hash_order_default<'a, I>(keys_in_insertion_order: I) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    java_hash_order_with_table(keys_in_insertion_order, DEFAULT_INITIAL_CAPACITY)
}

pub fn java_hash_order_with_capacity<'a, I>(keys_in_insertion_order: I, initial_capacity: usize) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    java_hash_order_with_table(keys_in_insertion_order, table_size_for(initial_capacity))
}

pub fn java_hash_set_copy_order<'a, I>(keys_in_insertion_order: I) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let keys: Vec<&'a str> = keys_in_insertion_order.into_iter().collect();
    let capacity = std::cmp::max((keys.len() as f32 / 0.75f32) as usize + 1, 16);
    java_hash_order_with_capacity(keys, capacity)
}

pub fn java_hash_map_order<'a, I>(keys: I) -> Vec<&'a String>
where
    I: IntoIterator<Item = &'a String>,
{
    let mut sorted: Vec<&'a String> = keys.into_iter().collect();
    sorted.sort();
    let order = java_hash_order_default(sorted.iter().map(|s| s.as_str()));
    order
        .into_iter()
        .filter_map(|k| sorted.iter().find(|s| s.as_str() == k).copied())
        .collect()
}
