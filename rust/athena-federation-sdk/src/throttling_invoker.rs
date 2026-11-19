use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::time::Instant;

use crate::data::BlockSpiller;
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};
use crate::java_cast::{double_to_long, long_to_double};

pub const THROTTLE_INITIAL_DELAY_MS: &str = "throttle_initial_delay_ms";
pub const THROTTLE_MAX_DELAY_MS: &str = "throttle_max_delay_ms";
pub const THROTTLE_DECREASE_FACTOR: &str = "throttle_decrease_factor";
pub const THROTTLE_INCREASE_MS: &str = "throttle_increase_ms";

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThrottleConfig {
    pub initial_delay_ms: i64,
    pub max_delay_ms: i64,
    pub decrease: f64,
    pub increase_ms: i64,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        ThrottleConfig {
            initial_delay_ms: 10,
            max_delay_ms: 1_000,
            decrease: 0.5,
            increase_ms: 10,
        }
    }
}

fn parse_long(value: &str) -> Result<i64> {
    value
        .parse::<i64>()
        .map_err(|_| FederationError::NumberFormat(format!("For input string: \"{value}\"")))
}

impl ThrottleConfig {
    pub fn from_config_options(config_options: &HashMap<String, String>) -> Result<Self> {
        let defaults = ThrottleConfig::default();
        let long_option = |key: &str, default: i64| {
            config_options
                .get(key)
                .map_or(Ok(default), |v| parse_long(v))
        };
        let initial_delay_ms = long_option(THROTTLE_INITIAL_DELAY_MS, defaults.initial_delay_ms)?;
        let max_delay_ms = long_option(THROTTLE_MAX_DELAY_MS, defaults.max_delay_ms)?;
        let decrease = match config_options.get(THROTTLE_DECREASE_FACTOR) {
            Some(v) => v
                .trim()
                .parse::<f64>()
                .map_err(|_| FederationError::NumberFormat(format!("For input string: \"{v}\"")))?,
            None => defaults.decrease,
        };
        Ok(ThrottleConfig {
            initial_delay_ms,
            max_delay_ms,
            decrease,
            increase_ms: long_option(THROTTLE_INCREASE_MS, defaults.increase_ms)?,
        })
    }

    fn validate(&self) -> Result<()> {
        if !(0.001..=1.0).contains(&self.decrease) {
            return Err(FederationError::invalid_input(format!(
                "decrease was {} but should be between .001 and 1",
                java_double(self.decrease)
            )));
        }
        if self.max_delay_ms < 1 {
            return Err(FederationError::invalid_input(format!(
                "maxDelayMs was {} but must be >= 1",
                self.max_delay_ms
            )));
        }
        if self.increase_ms < 1 {
            return Err(FederationError::invalid_input(format!(
                "increase was {} but must be >= 1",
                self.increase_ms
            )));
        }
        Ok(())
    }
}

struct Throttle {
    delay: i64,
    state: State,
}

pub struct ThrottlingInvoker<E = FederationError> {
    config: ThrottleConfig,
    filter: fn(&E) -> bool,
    spiller: Option<Arc<dyn BlockSpiller>>,
    throttle: Mutex<Throttle>,
}

fn java_double(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        value.to_string()
    }
}

impl<E> ThrottlingInvoker<E>
where
    E: From<FederationError> + fmt::Display + Send,
{
    pub fn new(
        config: ThrottleConfig,
        filter: fn(&E) -> bool,
        spiller: Option<Arc<dyn BlockSpiller>>,
    ) -> Result<ThrottlingInvoker<E>> {
        config.validate()?;
        Ok(ThrottlingInvoker {
            config,
            filter,
            spiller,
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
                    if !(self.filter)(&ex) {
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

    pub fn config(&self) -> &ThrottleConfig {
        &self.config
    }

    pub fn state(&self) -> State {
        self.lock_throttle().state
    }

    pub fn delay(&self) -> i64 {
        self.lock_throttle().delay
    }

    fn lock_throttle(&self) -> MutexGuard<'_, Throttle> {
        self.throttle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    async fn handle_throttle(&self, ex: &E) -> std::result::Result<(), E> {
        {
            let mut throttle = self.lock_throttle();
            let mut new_delay =
                double_to_long((long_to_double(throttle.delay) / self.config.decrease).ceil());
            if new_delay == 0 {
                new_delay = self.config.initial_delay_ms;
            } else if new_delay > self.config.max_delay_ms {
                new_delay = self.config.max_delay_ms;
            }
            tracing::info!(
                "handleThrottle: Encountered a Throttling event[{}] adjusting delay to {} ms @ {} TPS",
                ex,
                new_delay,
                1000f64 / long_to_double(new_delay)
            );
            throttle.state = State::Congested;
            throttle.delay = new_delay;
        }
        if let Some(spiller) = &self.spiller {
            if !spiller.spilled().await.map_err(E::from)? {
                return Err(E::from(FederationError::connector(
                    FederationSourceErrorCode::ThrottlingException,
                    format!("ThrottlingInvoker requesting slow down due to {ex}"),
                )));
            }
        }
        Ok(())
    }

    fn handle_avoidance(&self) {
        let mut throttle = self.lock_throttle();
        let new_delay = (throttle.delay - self.config.increase_ms).max(0);
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
        let delay = self.delay();
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay.unsigned_abs())).await;
        }
    }

    fn is_timed_out(start_time: Instant, timeout_millis: i64) -> bool {
        timeout_millis > 0
            && i64::try_from(start_time.elapsed().as_millis()).unwrap_or(i64::MAX) > timeout_millis
    }
}

impl<E> fmt::Display for ThrottlingInvoker<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let throttle = self
            .throttle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write!(
            f,
            "ThrottlingInvoker{{initialDelayMs={}, maxDelayMs={}, decrease={}, increase={}, delay={}, state={}}}",
            self.config.initial_delay_ms,
            self.config.max_delay_ms,
            java_double(self.config.decrease),
            self.config.increase_ms,
            throttle.delay,
            throttle.state
        )
    }
}
