use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::data::{Block, BlockSpiller, RowWriter};
use athena_federation_sdk::domain::predicate::ConstraintEvaluator;
use athena_federation_sdk::domain::spill::SpillLocation;
use athena_federation_sdk::exceptions::JavaThrowable;
use athena_federation_sdk::{
    exceptions::{FederationSourceErrorCode, FederationThrottleException},
    FederationError,
};
use athena_federation_sdk::{
    State, ThrottleConfig, ThrottlingInvoker, THROTTLE_DECREASE_FACTOR, THROTTLE_INCREASE_MS,
    THROTTLE_INITIAL_DELAY_MS, THROTTLE_MAX_DELAY_MS,
};

fn throttle_filter(ex: &FederationError) -> bool {
    matches!(ex, FederationError::Throttle(_))
}

fn throttling_error() -> FederationError {
    FederationError::connector(
        FederationSourceErrorCode::ThrottlingException,
        "Throttling error",
    )
}

struct MockSpiller;

#[async_trait]
impl BlockSpiller for MockSpiller {
    async fn write_rows(
        &self,
        _row_writer: &mut RowWriter<'_>,
    ) -> athena_federation_sdk::Result<()> {
        Ok(())
    }

    fn constraint_evaluator(&self) -> Arc<ConstraintEvaluator> {
        Arc::new(ConstraintEvaluator::empty_evaluator())
    }

    async fn spilled(&self) -> athena_federation_sdk::Result<bool> {
        Ok(false)
    }

    async fn get_block(&self) -> athena_federation_sdk::Result<Block> {
        Err(FederationError::internal("not used"))
    }

    async fn get_spill_locations(&self) -> athena_federation_sdk::Result<Vec<SpillLocation>> {
        Ok(Vec::new())
    }

    async fn close(&self) {}
}

#[tokio::test]
async fn invoke_no_throttle() {
    let invoker = ThrottlingInvoker::new(
        ThrottleConfig {
            initial_delay_ms: 10,
            max_delay_ms: 2_000,
            decrease: 0.5,
            increase_ms: 10,
        },
        throttle_filter,
        None,
    )
    .unwrap();

    for _ in 0..100 {
        let result = invoker
            .invoke_with_timeout_millis(|| async { Ok::<i32, FederationError>(1 + 1) }, 10_000)
            .await
            .unwrap();
        assert_eq!(2, result);
        assert_eq!(State::FastStart, invoker.state());
        assert_eq!(0, invoker.delay());
    }
}

#[tokio::test]
async fn invoke_with_throttle() {
    let invoker = ThrottlingInvoker::new(
        ThrottleConfig {
            initial_delay_ms: 10,
            max_delay_ms: 200,
            decrease: 0.8,
            increase_ms: 1,
        },
        throttle_filter,
        None,
    )
    .unwrap();

    for i in 0..5i64 {
        let count = Arc::new(AtomicI64::new(0));
        let val = i;
        let result = invoker
            .invoke_with_timeout_millis(
                || {
                    let count = count.clone();
                    async move {
                        if count.fetch_add(1, Ordering::SeqCst) + 1 < 4 {
                            return Err(FederationError::Throttle(
                                FederationThrottleException::new(),
                            ));
                        }
                        Ok(val)
                    }
                },
                10_000,
            )
            .await
            .unwrap();
        assert_eq!(val, result);
        assert_eq!(4, count.load(Ordering::SeqCst));
        assert_eq!(State::Avoidance, invoker.state());
        assert!(invoker.delay() > 0);
    }

    assert_eq!(199, invoker.delay());
}

#[tokio::test]
async fn invoke_with_throttle_timeout() {
    let invoker = ThrottlingInvoker::new(
        ThrottleConfig {
            initial_delay_ms: 10,
            max_delay_ms: 500,
            decrease: 0.5,
            increase_ms: 10,
        },
        throttle_filter,
        None,
    )
    .unwrap();

    let ex = invoker
        .invoke_with_timeout_millis(
            || async { Err::<i32, FederationError>(throttling_error()) },
            2_000,
        )
        .await
        .unwrap_err();
    assert!(ex.as_connector_exception().is_some());
}

#[tokio::test]
async fn invoke_with_throttle_no_spill() {
    let spiller: Arc<dyn BlockSpiller> = Arc::new(MockSpiller);
    let invoker = ThrottlingInvoker::new(
        ThrottleConfig {
            initial_delay_ms: 10,
            max_delay_ms: 500,
            decrease: 0.5,
            increase_ms: 10,
        },
        |ex: &FederationError| ex.is_runtime_exception(),
        Some(spiller),
    )
    .unwrap();

    let ex = invoker
        .invoke_with_timeout_millis(
            || async { Err::<i32, FederationError>(throttling_error()) },
            2_000,
        )
        .await
        .unwrap_err();
    let connector = ex
        .as_connector_exception()
        .expect("AthenaConnectorException");
    assert_eq!(
        Some(FederationSourceErrorCode::ThrottlingException),
        connector.error_code()
    );
    assert!(connector
        .message()
        .starts_with("ThrottlingInvoker requesting slow down due to"));
}

#[test]
fn test_default_builder_from_config() {
    let config_option = HashMap::from([
        (THROTTLE_INITIAL_DELAY_MS.to_string(), "100".to_string()),
        (THROTTLE_MAX_DELAY_MS.to_string(), "10000".to_string()),
        (THROTTLE_DECREASE_FACTOR.to_string(), "0.1".to_string()),
        (THROTTLE_INCREASE_MS.to_string(), "500".to_string()),
    ]);
    let invoker: ThrottlingInvoker = ThrottlingInvoker::new(
        ThrottleConfig::from_config_options(&config_option).unwrap(),
        throttle_filter,
        None,
    )
    .unwrap();
    let config = invoker.config();
    assert_eq!(0.1f64.to_bits(), config.decrease.to_bits());
    assert_eq!(500, config.increase_ms);
    assert_eq!(100, config.initial_delay_ms);
    assert_eq!(10000, config.max_delay_ms);
}
