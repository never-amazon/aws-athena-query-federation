use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use aws_credential_types::Credentials;
use tokio::task::JoinHandle;

use crate::security::{AwsService, AwsServiceError};
use crate::throttling_invoker::ThrottlingInvoker;

const FIBONACCI: [u64; 10] = [1, 1, 2, 3, 5, 8, 13, 21, 34, 55];
const TERMINAL_STATES: [&str; 3] = ["SUCCEEDED", "FAILED", "CANCELLED"];
const INVALID_REQUEST_EXCEPTION: &str = "InvalidRequestException";

#[async_trait]
pub trait AthenaClient: Send + Sync {
    async fn get_query_execution_state(
        &self,
        query_execution_id: &str,
    ) -> Result<Option<String>, AwsServiceError>;

    fn with_credentials(&self, credentials: &Credentials) -> Arc<dyn AthenaClient>;
}

#[async_trait]
impl AthenaClient for aws_sdk_athena::Client {
    async fn get_query_execution_state(
        &self,
        query_execution_id: &str,
    ) -> Result<Option<String>, AwsServiceError> {
        let output = self
            .get_query_execution()
            .query_execution_id(query_execution_id)
            .send()
            .await
            .map_err(|e| AwsServiceError::from_sdk_error(AwsService::Athena, &e))?;
        Ok(output
            .query_execution()
            .and_then(|q| q.status())
            .and_then(|s| s.state())
            .map(|s| s.as_str().to_string()))
    }

    fn with_credentials(&self, credentials: &Credentials) -> Arc<dyn AthenaClient> {
        let config = self
            .config()
            .to_builder()
            .credentials_provider(credentials.clone())
            .build();
        Arc::new(aws_sdk_athena::Client::from_conf(config))
    }
}

pub struct QueryStatusChecker {
    is_running: Arc<AtomicBool>,
    athena: Arc<dyn AthenaClient>,
    athena_invoker: Arc<ThrottlingInvoker<AwsServiceError>>,
    query_id: String,
    checker: Mutex<Option<JoinHandle<()>>>,
    was_started: AtomicBool,
}

impl QueryStatusChecker {
    pub fn new(
        athena: Arc<dyn AthenaClient>,
        athena_invoker: Arc<ThrottlingInvoker<AwsServiceError>>,
        query_id: impl Into<String>,
    ) -> QueryStatusChecker {
        QueryStatusChecker {
            is_running: Arc::new(AtomicBool::new(true)),
            athena,
            athena_invoker,
            query_id: query_id.into(),
            checker: Mutex::new(None),
            was_started: AtomicBool::new(false),
        }
    }

    pub fn is_query_running(&self) -> bool {
        if !self.was_started.load(Ordering::SeqCst) {
            let mut checker = self.lock_checker();
            if !self.was_started.load(Ordering::SeqCst) {
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    let task = StatusTask {
                        is_running: self.is_running.clone(),
                        athena: self.athena.clone(),
                        athena_invoker: self.athena_invoker.clone(),
                        query_id: self.query_id.clone(),
                    };
                    *checker = Some(handle.spawn(task.run()));
                } else {
                    tracing::warn!("No async runtime available. Query status polling is disabled");
                }
                self.was_started.store(true, Ordering::SeqCst);
            }
        }
        self.is_running.load(Ordering::SeqCst)
    }

    pub fn close(&self) {
        if let Some(checker) = self.lock_checker().as_ref() {
            checker.abort();
        }
        tracing::debug!("Interrupt signal sent to status checker thread");
    }

    fn lock_checker(&self) -> MutexGuard<'_, Option<JoinHandle<()>>> {
        self.checker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for QueryStatusChecker {
    fn drop(&mut self) {
        self.close();
    }
}

struct StatusTask {
    is_running: Arc<AtomicBool>,
    athena: Arc<dyn AthenaClient>,
    athena_invoker: Arc<ThrottlingInvoker<AwsServiceError>>,
    query_id: String,
}

impl StatusTask {
    async fn run(self) {
        let mut attempt = 0usize;
        while self.is_running.load(Ordering::SeqCst) {
            let delay = FIBONACCI[attempt.min(FIBONACCI.len() - 1)];
            tokio::time::sleep(Duration::from_secs(delay)).await;
            if !self.check_status(attempt).await {
                tracing::debug!("Checker thread interrupted. Ceasing status polling");
                return;
            }
            attempt += 1;
        }
        tracing::debug!("Query terminated. Ceasing status polling");
    }

    async fn check_status(&self, attempt: usize) -> bool {
        tracing::debug!(
            "Background thread checking status of Athena query {}, attempt {}",
            self.query_id,
            attempt
        );
        let athena = self.athena.clone();
        let query_id = self.query_id.clone();
        let result = self
            .athena_invoker
            .invoke(|| {
                let athena = athena.clone();
                let query_id = query_id.clone();
                async move { athena.get_query_execution_state(&query_id).await }
            })
            .await;
        match result {
            Ok(state) => {
                let state = state.unwrap_or_else(|| "null".to_string());
                if TERMINAL_STATES.contains(&state.as_str()) {
                    tracing::debug!(
                        "Query {} has terminated with state {}",
                        self.query_id,
                        state
                    );
                    self.is_running.store(false, Ordering::SeqCst);
                }
                true
            }
            Err(e) => {
                tracing::warn!(
                    "Exception {} thrown when calling Athena for query status: {}",
                    e.error_code().unwrap_or("AwsServiceException"),
                    e.message()
                );
                if e.is_error_code(INVALID_REQUEST_EXCEPTION) {
                    tracing::debug!(
                        "Athena reports query {} not found. Interrupting checker thread",
                        self.query_id
                    );
                    return false;
                }
                true
            }
        }
    }
}
