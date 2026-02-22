use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct NoOpOperator;

impl NoOpOperator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Operator for NoOpOperator {
    fn name(&self) -> &'static str {
        "NoOpOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        if !params.is_object() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "NoOpOperator params must be an object",
            ));
        }
        Ok(())
    }

    async fn execute(&self, _params: Value, _ctx: ExecutionContext) -> Result<Value, AppError> {
        Ok(json!({"status": "ok"}))
    }
}
