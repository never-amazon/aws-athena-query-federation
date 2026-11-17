use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::data::{Block, BlockSpiller, RowWriter};
use athena_federation_sdk::domain::predicate::ConstraintEvaluator;
use athena_federation_sdk::domain::spill::SpillLocation;
use athena_federation_sdk::{
    exceptions::{FederationSourceErrorCode, FederationThrottleException},
    FederationError,
};
use athena_federation_sdk::{
    ExceptionFilter, State, ThrottlingInvoker, THROTTLE_DECREASE_FACTOR, THROTTLE_INCREASE_MS,
    THROTTLE_INITIAL_DELAY_MS, THROTTLE_MAX_DELAY_MS,
};

fn throttle_filter() -> Arc<dyn ExceptionFilter<FederationError>> {
    Arc::new(|ex: &FederationError| matches!(ex, FederationError::Throttle(_)))
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
        _row_writer: &mut (dyn RowWriter + Send),
    ) -> athena_federation_sdk::Result<()> {
        Ok(())
    }

    fn get_constraint_evaluator(&self) -> Arc<ConstraintEvaluator> {
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
    let invoker = ThrottlingInvoker::builder()
        .with_decrease(0.5)
        .with_increase(10)
        .with_initial_delay_ms(10)
        .with_max_delay_ms(2_000)
        .with_filter(throttle_filter())
        .build()
        .unwrap();

    for _ in 0..100 {
        let result = invoker
            .invoke_with_timeout_millis(|| async { Ok::<i32, FederationError>(1 + 1) }, 10_000)
            .await
            .unwrap();
        assert_eq!(2, result);
        assert_eq!(State::FastStart, invoker.get_state());
        assert_eq!(0, invoker.get_delay());
    }
}

#[tokio::test]
async fn invoke_with_throttle() {
    let invoker = ThrottlingInvoker::builder()
        .with_decrease(0.8)
        .with_increase(1)
        .with_initial_delay_ms(10)
        .with_max_delay_ms(200)
        .with_filter(throttle_filter())
        .build()
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
        assert_eq!(State::Avoidance, invoker.get_state());
        assert!(invoker.get_delay() > 0);
    }

    assert_eq!(199, invoker.get_delay());
}

#[tokio::test]
async fn invoke_with_throttle_timeout() {
    let invoker = ThrottlingInvoker::builder()
        .with_decrease(0.5)
        .with_increase(10)
        .with_initial_delay_ms(10)
        .with_max_delay_ms(500)
        .with_filter(throttle_filter())
        .build()
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
    let invoker = ThrottlingInvoker::builder()
        .with_decrease(0.5)
        .with_increase(10)
        .with_initial_delay_ms(10)
        .with_max_delay_ms(500)
        .with_filter(Arc::new(|ex: &FederationError| ex.is_runtime_exception()))
        .with_spiller(spiller)
        .build()
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
        .get_message()
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
    let builder =
        ThrottlingInvoker::new_default_builder(throttle_filter(), &config_option).unwrap();
    let invoker = builder.build().unwrap();
    assert_eq!(0.1f64.to_bits(), invoker.get_decrease().to_bits());
    assert_eq!(500, invoker.get_increase());
    assert_eq!(100, invoker.get_default_initial_delay_ms());
    assert_eq!(10000, invoker.get_max_delay_ms());
}
