use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::time::Instant;

use crate::data::BlockSpiller;
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};

pub const THROTTLE_INITIAL_DELAY_MS: &str = "throttle_initial_delay_ms";
pub const THROTTLE_MAX_DELAY_MS: &str = "throttle_max_delay_ms";
pub const THROTTLE_DECREASE_FACTOR: &str = "throttle_decrease_factor";
pub const THROTTLE_INCREASE_MS: &str = "throttle_increase_ms";

const DEFAULT_INITIAL_DELAY_MS: i64 = 10;
const DEFAULT_MAX_DELAY_MS: i64 = 1_000;
const DEFAULT_DECREASE_FACTOR: f64 = 0.5;
const DEFAULT_INCREASE_MS: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    FastStart,
    Congested,
    Avoidance,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            State::FastStart => "FAST_START",
            State::Congested => "CONGESTED",
            State::Avoidance => "AVOIDANCE",
        })
    }
}

pub trait ExceptionFilter<E>: Send + Sync {
    fn is_match(&self, ex: &E) -> bool;
}

impl<E, F> ExceptionFilter<E> for F
where
    F: Fn(&E) -> bool + Send + Sync,
{
    fn is_match(&self, ex: &E) -> bool {
        self(ex)
    }
}

struct Throttle {
    delay: i64,
    state: State,
}

pub struct ThrottlingInvoker<E = FederationError> {
    initial_delay_ms: i64,
    max_delay_ms: i64,
    decrease: f64,
    increase: i64,
    filter: Arc<dyn ExceptionFilter<E>>,
    spiller: Mutex<Option<Arc<dyn BlockSpiller>>>,
    throttle: Mutex<Throttle>,
}

fn java_double(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{:.1}", value)
    } else {
        value.to_string()
    }
}

fn parse_long(value: &str) -> Result<i64> {
    value
        .parse::<i64>()
        .map_err(|_| FederationError::Runtime(format!("For input string: \"{}\"", value)))
}

impl<E> ThrottlingInvoker<E>
where
    E: From<FederationError> + fmt::Display + Send,
{
    pub fn builder() -> ThrottlingInvokerBuilder<E> {
        ThrottlingInvokerBuilder::default()
    }

    pub fn new_default_builder(
        filter: Arc<dyn ExceptionFilter<E>>,
        config_options: &HashMap<String, String>,
    ) -> Result<ThrottlingInvokerBuilder<E>> {
        let initial_delay_ms = match config_options.get(THROTTLE_INITIAL_DELAY_MS) {
            Some(v) => parse_long(v)?,
            None => DEFAULT_INITIAL_DELAY_MS,
        };
        let max_delay_ms = match config_options.get(THROTTLE_MAX_DELAY_MS) {
            Some(v) => parse_long(v)?,
            None => DEFAULT_MAX_DELAY_MS,
        };
        let decrease_factor = match config_options.get(THROTTLE_DECREASE_FACTOR) {
            Some(v) => v
                .trim()
                .parse::<f64>()
                .map_err(|_| FederationError::Runtime(format!("For input string: \"{}\"", v)))?,
            None => DEFAULT_DECREASE_FACTOR,
        };
        let increase = match config_options.get(THROTTLE_INCREASE_MS) {
            Some(v) => parse_long(v)?,
            None => DEFAULT_INCREASE_MS,
        };
        Ok(ThrottlingInvoker::builder()
            .with_initial_delay_ms(initial_delay_ms)
            .with_max_delay_ms(max_delay_ms)
            .with_decrease(decrease_factor)
            .with_increase(increase)
            .with_filter(filter))
    }

    pub fn new(
        initial_delay_ms: i64,
        max_delay_ms: i64,
        decrease: f64,
        increase: i64,
        filter: Arc<dyn ExceptionFilter<E>>,
        spiller: Option<Arc<dyn BlockSpiller>>,
    ) -> Result<ThrottlingInvoker<E>> {
        if !(0.001..=1.0).contains(&decrease) {
            return Err(FederationError::invalid_input(format!(
                "decrease was {} but should be between .001 and 1",
                java_double(decrease)
            )));
        }
        if max_delay_ms < 1 {
            return Err(FederationError::invalid_input(format!(
                "maxDelayMs was {} but must be >= 1",
                max_delay_ms
            )));
        }
        if increase < 1 {
            return Err(FederationError::invalid_input(format!(
                "increase was {} but must be >= 1",
                increase
            )));
        }
        Ok(ThrottlingInvoker {
            initial_delay_ms,
            max_delay_ms,
            decrease,
            increase,
            filter,
            spiller: Mutex::new(spiller),
            throttle: Mutex::new(Throttle {
                delay: 0,
                state: State::FastStart,
            }),
        })
    }

    pub async fn invoke<T, F, Fut>(&self, callable: F) -> std::result::Result<T, E>
    where
        F: FnMut() -> Fut + Send,
        Fut: Future<Output = std::result::Result<T, E>> + Send,
    {
        self.invoke_with_timeout_millis(callable, 0).await
    }

    pub async fn invoke_with_timeout_millis<T, F, Fut>(
        &self,
        mut callable: F,
        timeout_millis: i64,
    ) -> std::result::Result<T, E>
    where
        F: FnMut() -> Fut + Send,
        Fut: Future<Output = std::result::Result<T, E>> + Send,
    {
        let start_time = Instant::now();
        loop {
            self.apply_sleep().await;
            match callable().await {
                Ok(result) => {
                    self.handle_avoidance();
                    return Ok(result);
                }
                Err(ex) => {
                    if !self.filter.is_match(&ex) {
                        return Err(ex);
                    }
                    self.handle_throttle(&ex).await?;
                }
            }
            if Self::is_timed_out(start_time, timeout_millis) {
                break;
            }
        }
        Err(E::from(FederationError::connector(
            FederationSourceErrorCode::OperationTimeoutException,
            format!(
                "Timed out before call succeeded after {} ms",
                start_time.elapsed().as_millis()
            ),
        )))
    }

    pub fn set_block_spiller(&self, spiller: Option<Arc<dyn BlockSpiller>>) {
        *self.lock_spiller() = spiller;
    }

    pub fn get_state(&self) -> State {
        self.lock_throttle().state
    }

    pub fn get_delay(&self) -> i64 {
        self.lock_throttle().delay
    }

    pub fn get_decrease(&self) -> f64 {
        self.decrease
    }

    pub fn get_default_initial_delay_ms(&self) -> i64 {
        self.initial_delay_ms
    }

    pub fn get_max_delay_ms(&self) -> i64 {
        self.max_delay_ms
    }

    pub fn get_increase(&self) -> i64 {
        self.increase
    }

    fn lock_throttle(&self) -> MutexGuard<'_, Throttle> {
        self.throttle.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_spiller(&self) -> MutexGuard<'_, Option<Arc<dyn BlockSpiller>>> {
        self.spiller.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn handle_throttle(&self, ex: &E) -> std::result::Result<(), E> {
        {
            let mut throttle = self.lock_throttle();
            let mut new_delay = (throttle.delay as f64 / self.decrease).ceil() as i64;
            if new_delay == 0 {
                new_delay = self.initial_delay_ms;
            } else if new_delay > self.max_delay_ms {
                new_delay = self.max_delay_ms;
            }
            tracing::info!(
                "handleThrottle: Encountered a Throttling event[{}] adjusting delay to {} ms @ {} TPS",
                ex,
                new_delay,
                1000f64 / new_delay as f64
            );
            throttle.state = State::Congested;
            throttle.delay = new_delay;
        }
        let spiller = self.lock_spiller().clone();
        if let Some(spiller) = spiller {
            if !spiller.spilled().await.map_err(E::from)? {
                return Err(E::from(FederationError::connector(
                    FederationSourceErrorCode::ThrottlingException,
                    format!("ThrottlingInvoker requesting slow down due to {}", ex),
                )));
            }
        }
        Ok(())
    }

    fn handle_avoidance(&self) {
        let mut throttle = self.lock_throttle();
        let new_delay = (throttle.delay - self.increase).max(0);
        if throttle.delay > 0 {
            throttle.state = State::Avoidance;
            if new_delay > 0 {
                tracing::info!(
                    "handleAvoidance: Congestion AVOIDANCE active, decreasing delay to {} ms @ {} TPS",
                    new_delay,
                    1000 / new_delay
                );
            } else {
                tracing::info!(
                    "handleAvoidance: Congestion AVOIDANCE active, decreasing delay to {} ms @ unlimited TPS",
                    new_delay
                );
            }
            throttle.delay = new_delay;
        }
    }

    async fn apply_sleep(&self) {
        let delay = self.get_delay();
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay as u64)).await;
        }
    }

    fn is_timed_out(start_time: Instant, timeout_millis: i64) -> bool {
        timeout_millis > 0 && start_time.elapsed().as_millis() as i64 > timeout_millis
    }
}

impl<E> fmt::Display for ThrottlingInvoker<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let throttle = self.throttle.lock().unwrap_or_else(|e| e.into_inner());
        write!(
            f,
            "ThrottlingInvoker{{initialDelayMs={}, maxDelayMs={}, decrease={}, increase={}, delay={}, state={}}}",
            self.initial_delay_ms,
            self.max_delay_ms,
            java_double(self.decrease),
            self.increase,
            throttle.delay,
            throttle.state
        )
    }
}

pub struct ThrottlingInvokerBuilder<E = FederationError> {
    initial_delay_ms: i64,
    max_delay_ms: i64,
    decrease: f64,
    increase: i64,
    filter: Option<Arc<dyn ExceptionFilter<E>>>,
    spiller: Option<Arc<dyn BlockSpiller>>,
}

impl<E> Default for ThrottlingInvokerBuilder<E> {
    fn default() -> Self {
        ThrottlingInvokerBuilder {
            initial_delay_ms: 0,
            max_delay_ms: 0,
            decrease: 0.0,
            increase: 0,
            filter: None,
            spiller: None,
        }
    }
}

impl<E> ThrottlingInvokerBuilder<E>
where
    E: From<FederationError> + fmt::Display + Send,
{
    pub fn with_initial_delay_ms(mut self, initial_delay_ms: i64) -> Self {
        self.initial_delay_ms = initial_delay_ms;
        self
    }

    pub fn with_max_delay_ms(mut self, max_delay_ms: i64) -> Self {
        self.max_delay_ms = max_delay_ms;
        self
    }

    pub fn with_decrease(mut self, decrease: f64) -> Self {
        self.decrease = decrease;
        self
    }

    pub fn with_increase(mut self, increase: i64) -> Self {
        self.increase = increase;
        self
    }

    pub fn with_filter(mut self, filter: Arc<dyn ExceptionFilter<E>>) -> Self {
        self.filter = Some(filter);
        self
    }

    pub fn with_spiller(mut self, spiller: Arc<dyn BlockSpiller>) -> Self {
        self.spiller = Some(spiller);
        self
    }

    pub fn build(self) -> Result<ThrottlingInvoker<E>> {
        let filter = self
            .filter
            .ok_or_else(|| FederationError::NullPointer("filter was null".into()))?;
        ThrottlingInvoker::new(
            self.initial_delay_ms,
            self.max_delay_ms,
            self.decrease,
            self.increase,
            filter,
            self.spiller,
        )
    }
}
