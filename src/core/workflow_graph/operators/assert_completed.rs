use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use serde_json::{json, Map, Value};

pub struct AssertCompletedOperator;

impl Default for AssertCompletedOperator {
    fn default() -> Self {
        Self::new()
    }
}

impl AssertCompletedOperator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Operator for AssertCompletedOperator {
    fn name(&self) -> &'static str {
        "AssertCompletedOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let require = params.get("require");
        if require.is_none() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "AssertCompletedOperator requires a 'require' array",
            ));
        }
        let arr = require.unwrap().as_array();
        if arr.is_none() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "'require' must be an array of task ids",
            ));
        }
        Ok(())
    }

    async fn execute(&self, params: Value, ctx: ExecutionContext) -> Result<Value, AppError> {
        let require = params
            .get("require")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AppError::new(ErrorCategory::ValidationError, "require must be an array")
            })?;
        let mut task_ids = Vec::new();
        for value in require {
            let id = value.as_str().ok_or_else(|| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    "require entries must be strings",
                )
            })?;
            task_ids.push(id.to_string());
        }

        let empty = Map::new();
        let tasks_map = ctx.state_view.tasks.as_object().unwrap_or(&empty);
        let mut statuses = Map::new();
        let mut all_succeeded = true;

        for task_id in task_ids.iter() {
            let status = tasks_map
                .get(task_id)
                .and_then(Value::as_object)
                .and_then(|details| details.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("missing");
            if status != "success" {
                all_succeeded = false;
            }
            statuses.insert(task_id.clone(), Value::String(status.to_string()));
            if status == "missing" {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("task {} is not yet completed", task_id),
                )
                .with_code("WFG-ASSERT-001"));
            }
        }

        Ok(json!({
            "all_succeeded": all_succeeded,
            "statuses": Value::Object(statuses),
        }))
    }
}
