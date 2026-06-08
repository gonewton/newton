use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct SetContextParams {
    pub patch: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SetContextOutput {
    pub applied: bool,
    pub patch: serde_json::Value,
}

pub struct SetContextOperator;

impl Default for SetContextOperator {
    fn default() -> Self {
        Self::new()
    }
}

impl SetContextOperator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Operator for SetContextOperator {
    fn name(&self) -> &'static str {
        "SetContextOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let patch = params.get("patch");
        if patch.is_none() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "SetContextOperator requires a patch object",
            ));
        }
        if !patch.unwrap().is_object() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "patch must be an object",
            ));
        }
        Ok(())
    }

    fn params_schema(&self) -> schemars::Schema {
        schemars::schema_for!(SetContextParams)
    }

    fn output_schema(&self) -> schemars::Schema {
        schemars::schema_for!(SetContextOutput)
    }

    async fn execute(&self, params: Value, _ctx: ExecutionContext) -> Result<Value, AppError> {
        let patch = params
            .get("patch")
            .cloned()
            .ok_or_else(|| AppError::new(ErrorCategory::ValidationError, "patch is required"))?;
        if !patch.is_object() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "patch must be an object",
            ));
        }
        Ok(json!({"applied": true, "patch": patch}))
    }
}
