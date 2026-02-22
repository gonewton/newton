use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use serde_json::{json, Value};

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
