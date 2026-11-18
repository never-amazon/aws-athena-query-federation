use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use athena_federation_sdk::security::AwsServiceError;
use athena_federation_sdk::{AthenaClient, ExceptionFilter, QueryStatusChecker, ThrottlingInvoker};
use aws_credential_types::Credentials;

type Response = Result<Option<String>, AwsServiceError>;

struct MockAthena {
    query_id: &'static str,
    responses: Mutex<Vec<Response>>,
    calls: AtomicUsize,
}

impl MockAthena {
    fn new(query_id: &'static str, responses: Vec<Response>) -> Arc<MockAthena> {
        Arc::new(MockAthena {
            query_id,
            responses: Mutex::new(responses),
            calls: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AthenaClient for MockAthena {
    async fn get_query_execution_state(&self, query_execution_id: &str) -> Response {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(self.query_id, query_execution_id);
        let mut responses = self.responses.lock().unwrap();
        if responses.len() > 1 {
            responses.remove(0)
        } else {
            responses[0].clone()
        }
    }

    fn with_credentials(&self, _credentials: &Credentials) -> Arc<dyn AthenaClient> {
        unreachable!("this test does not pass FAS credentials")
    }
}

fn athena_invoker() -> Arc<ThrottlingInvoker<AwsServiceError>> {
    let athena_exception_filter: Arc<dyn ExceptionFilter<AwsServiceError>> =
        Arc::new(|ex: &AwsServiceError| ex.is_error_code("TooManyRequestsException"));
    Arc::new(
        ThrottlingInvoker::new_default_builder(athena_exception_filter, &HashMap::new())
            .unwrap()
            .build()
            .unwrap(),
    )
}

fn state(s: &str) -> String {
    s.to_string()
}

#[tokio::test(start_paused = true)]
async fn test_fast_termination() {
    let athena = MockAthena::new("query0", vec![Ok(Some(state("FAILED")))]);
    let query_status_checker = QueryStatusChecker::new(athena.clone(), athena_invoker(), "query0");
    assert!(query_status_checker.is_query_running());
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(!query_status_checker.is_query_running());
    assert_eq!(1, athena.calls());
    drop(query_status_checker);
}

#[tokio::test(start_paused = true)]
async fn test_slow_termination() {
    let athena = MockAthena::new(
        "query1",
        vec![
            Ok(Some(state("RUNNING"))),
            Ok(Some(state("RUNNING"))),
            Ok(Some(state("SUCCEEDED"))),
        ],
    );
    let query_status_checker = QueryStatusChecker::new(athena.clone(), athena_invoker(), "query1");
    assert!(query_status_checker.is_query_running());
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(query_status_checker.is_query_running());
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(!query_status_checker.is_query_running());
    assert_eq!(3, athena.calls());
    drop(query_status_checker);
}

#[tokio::test(start_paused = true)]
async fn test_not_found() {
    let athena = MockAthena::new(
        "query2",
        vec![Err(AwsServiceError::with_code(
            "InvalidRequestException",
            "",
        ))],
    );
    let query_status_checker = QueryStatusChecker::new(athena.clone(), athena_invoker(), "query2");
    assert!(query_status_checker.is_query_running());
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(query_status_checker.is_query_running());
    assert_eq!(1, athena.calls());
    drop(query_status_checker);
}

#[tokio::test(start_paused = true)]
async fn test_other_error() {
    let athena = MockAthena::new("query3", vec![Err(AwsServiceError::new(""))]);
    let query_status_checker = QueryStatusChecker::new(athena.clone(), athena_invoker(), "query3");
    assert!(query_status_checker.is_query_running());
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(query_status_checker.is_query_running());
    assert_eq!(2, athena.calls());
    drop(query_status_checker);
}
