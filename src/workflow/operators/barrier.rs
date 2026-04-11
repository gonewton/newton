#![allow(clippy::result_large_err)] // BarrierOperator returns AppError directly for structured diagnostics without boxing.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use crate::workflow::schema::BarrierParams;
use async_trait::async_trait;
use serde_json::Value;

/// Barrier operator that waits for multiple tasks to complete before proceeding.
/// This operator becomes ready when all task IDs in its `expected` list have completed.
pub struct BarrierOperator {}

impl BarrierOperator {
    pub fn new() -> Self {
        BarrierOperator {}
    }
}

#[async_trait]
impl Operator for BarrierOperator {
    fn name(&self) -> &'static str {
        "barrier"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let _barrier_params: BarrierParams =
            serde_json::from_value(params.clone()).map_err(|err| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    format!("Invalid barrier operator parameters: {}", err),
                )
            })?;

        Ok(())
    }

    async fn execute(&self, params: Value, _ctx: ExecutionContext) -> Result<Value, AppError> {
        let barrier_params: BarrierParams = serde_json::from_value(params).map_err(|err| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("Invalid barrier operator parameters: {}", err),
            )
        })?;

        // The actual barrier logic is handled by the scheduler in the executor.
        // When this execute method is called, it means all expected tasks have completed.
        // Return information about what tasks we waited for.
        Ok(serde_json::json!({
            "expected_tasks": barrier_params.expected,
            "barrier_passed": true,
            "message": format!("Barrier passed: {} task(s) completed", barrier_params.expected.len())
        }))
    }
}

impl Default for BarrierOperator {
    fn default() -> Self {
        Self::new()
    }
}
