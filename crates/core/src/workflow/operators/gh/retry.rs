use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use serde_json::{Map, Value};
use std::time::Duration;
use tokio::time::sleep;

pub const MAX_RETRY_DELAY_MS: u64 = 300_000;

pub struct RetryConfig {
    pub count: usize,
    pub initial_delay_ms: u64,
    pub multiplier: f32,
    pub jitter_ms: u64,
}

impl RetryConfig {
    pub fn from_map(map: &Map<String, Value>) -> Self {
        Self {
            count: map.get("retry_count").and_then(Value::as_i64).unwrap_or(3) as usize,
            initial_delay_ms: map
                .get("retry_delay_ms")
                .and_then(Value::as_i64)
                .unwrap_or(5000) as u64,
            multiplier: map
                .get("retry_multiplier")
                .and_then(Value::as_f64)
                .unwrap_or(2.0) as f32,
            jitter_ms: map
                .get("retry_jitter_ms")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        }
    }

    pub fn validate(map: &Map<String, Value>) -> Result<(), AppError> {
        if let Some(retry_count) = map.get("retry_count").and_then(Value::as_i64) {
            if retry_count < 1 {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "retry_count must be at least 1",
                ));
            }
        }
        if let Some(delay) = map.get("retry_delay_ms").and_then(Value::as_i64) {
            if delay < 0 {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "retry_delay_ms must be non-negative",
                ));
            }
        }
        if let Some(mult) = map.get("retry_multiplier").and_then(Value::as_f64) {
            if mult < 1.0 {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "retry_multiplier must be >= 1.0",
                ));
            }
        }
        if let Some(jitter) = map.get("retry_jitter_ms").and_then(Value::as_i64) {
            if jitter < 0 {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "retry_jitter_ms must be non-negative",
                ));
            }
        }
        Ok(())
    }

    pub fn start_delay_ms(&self) -> u64 {
        self.initial_delay_ms.min(MAX_RETRY_DELAY_MS)
    }

    pub async fn backoff(&self, attempt: usize, delay_ms: &mut u64, label: &str) {
        if attempt < self.count {
            use rand::Rng;
            let jitter = if self.jitter_ms > 0 {
                rand::thread_rng().gen_range(0..=self.jitter_ms)
            } else {
                0
            };
            let sleep_ms = delay_ms.saturating_add(jitter).min(MAX_RETRY_DELAY_MS);
            tracing::warn!(
                attempt,
                max_attempts = self.count,
                delay_ms = sleep_ms,
                "{label} failed, retrying after delay"
            );
            sleep(Duration::from_millis(sleep_ms)).await;
            *delay_ms =
                ((*delay_ms as f32) * self.multiplier).min(MAX_RETRY_DELAY_MS as f32) as u64;
        }
    }
}
