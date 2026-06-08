use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema, Default)]
pub struct NoOpParams {}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct NoOpOutput {
    pub status: String,
}

pub struct NoOpOperator;

impl Default for NoOpOperator {
    fn default() -> Self {
        Self::new()
    }
}

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

    fn params_schema(&self) -> schemars::Schema {
        schemars::schema_for!(NoOpParams)
    }

    fn output_schema(&self) -> schemars::Schema {
        schemars::schema_for!(NoOpOutput)
    }

    async fn execute(&self, _params: Value, _ctx: ExecutionContext) -> Result<Value, AppError> {
        Ok(json!({"status": "ok"}))
    }
}
