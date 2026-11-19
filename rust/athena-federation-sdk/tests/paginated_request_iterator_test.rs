use std::collections::HashMap;

use athena_federation_sdk::util::PaginatedRequestIterator;

fn page(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn fake_page_request(page_token: Option<&str>) -> Option<HashMap<String, String>> {
    match page_token {
        None => Some(page(&[("data", "aaa"), ("nextPage", "2")])),
        Some("2") => Some(page(&[("data", "bbb"), ("nextPage", "3")])),
        Some("3") => Some(page(&[("data", "ccc")])),
        _ => None,
    }
}

fn get_page_token(input: Option<&HashMap<String, String>>) -> Option<String> {
    input.and_then(|m| m.get("nextPage").cloned())
}

#[test]
fn test_iterates_over_all_pages() {
    let result: Vec<String> = PaginatedRequestIterator::new(fake_page_request, |p: &Option<_>| {
        get_page_token(p.as_ref())
    })
    .map(|r| r.and_then(|m| m.get("data").cloned()).unwrap_or_default())
    .collect();
    assert_eq!(vec!["aaa", "bbb", "ccc"], result);
}

#[test]
fn test_behavior() {
    let mut iterator = PaginatedRequestIterator::new(fake_page_request, |p: &Option<_>| {
        get_page_token(p.as_ref())
    });
    assert_eq!(
        Some(Some(page(&[("data", "aaa"), ("nextPage", "2")]))),
        iterator.next()
    );
    assert_eq!(
        Some(Some(page(&[("data", "bbb"), ("nextPage", "3")]))),
        iterator.next()
    );
    assert_eq!(Some(Some(page(&[("data", "ccc")]))), iterator.next());
    assert_eq!(None, iterator.next());
    assert_eq!(None, iterator.next());
}
