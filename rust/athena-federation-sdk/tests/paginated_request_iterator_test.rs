use std::collections::HashMap;

use athena_federation_sdk::util::PaginatedRequestIterator;

fn page(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn fake_page_request(page_token: Option<&str>) -> Option<HashMap<String, String>> {
    match page_token {
        None => Some(page(&[("data", "aaa"), ("nextPage", "2")])),
        Some("2") => Some(page(&[("data", "bbb"), ("nextPage", "3")])),
        Some("3") => Some(page(&[("data", "ccc")])),
        _ => None,
    }
}

fn get_page_token(input: &Option<HashMap<String, String>>) -> Option<String> {
    input.as_ref().and_then(|m| m.get("nextPage").cloned())
}

#[test]
fn test_iterates_over_all_pages() {
    let result: Vec<String> = PaginatedRequestIterator::stream(fake_page_request, get_page_token)
        .map(|r| r.and_then(|m| m.get("data").cloned()).unwrap_or_default())
        .collect();
    assert_eq!(vec!["aaa", "bbb", "ccc"], result);
}

#[test]
fn test_behavior() {
    let mut iterator = PaginatedRequestIterator::new(fake_page_request, get_page_token);
    assert!(iterator.has_next());
    for _ in 0..10 {
        assert!(iterator.has_next());
    }
    assert_eq!(Some(page(&[("data", "aaa"), ("nextPage", "2")])), iterator.next_page().unwrap());
    for _ in 0..10 {
        assert!(iterator.has_next());
    }
    assert_eq!(Some(page(&[("data", "bbb"), ("nextPage", "3")])), iterator.next_page().unwrap());
    assert!(iterator.has_next());
    for _ in 0..10 {
        assert!(iterator.has_next());
    }
    assert_eq!(Some(page(&[("data", "ccc")])), iterator.next_page().unwrap());
    assert!(!iterator.has_next());
    assert!(iterator.next_page().unwrap_err().as_connector_exception().is_some());
    assert!(!iterator.has_next());
}
