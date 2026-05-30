use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use serde_json::{json, Map, Value};

use super::utils::resolve_option_id;
use super::GhOperator;

pub(super) fn validate_project_item_set_status(map: &Map<String, Value>) -> Result<(), AppError> {
    if map
        .get("item_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "item_id is required for project_item_set_status",
        ));
    }
    if map.get("board").and_then(Value::as_object).is_none() {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "board is required for project_item_set_status",
        ));
    }

    let has_explicit = map
        .get("single_select_option_id")
        .or_else(|| map.get("option_id"))
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty());
    let status = map.get("status").and_then(Value::as_str).unwrap_or("");
    if !has_explicit && status.is_empty() {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "project_item_set_status requires status or single_select_option_id (or option_id)",
        ));
    }
    Ok(())
}

impl GhOperator {
    pub(super) async fn execute_project_item_set_status(
        &self,
        map: &Map<String, Value>,
        workspace: &std::path::Path,
    ) -> Result<Value, AppError> {
        let item_id = map.get("item_id").and_then(Value::as_str).unwrap();
        let board = map.get("board").and_then(Value::as_object).unwrap();
        let status = map.get("status").and_then(Value::as_str).unwrap_or("");
        let on_error = map
            .get("on_error")
            .and_then(Value::as_str)
            .unwrap_or("warn");

        let option_id = match map
            .get("single_select_option_id")
            .or_else(|| map.get("option_id"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            Some(id) => id.to_string(),
            None => resolve_option_id(board, status)?,
        };

        let project_id = board["project_id"].as_str().ok_or_else(|| {
            AppError::new(ErrorCategory::ValidationError, "board missing project_id")
        })?;

        let field_id = board["field_id"].as_str().ok_or_else(|| {
            AppError::new(ErrorCategory::ValidationError, "board missing field_id")
        })?;

        let mut last_error: Option<AppError> = None;
        for attempt in 1..=2 {
            let result = self
                .runner
                .run(
                    &[
                        "project",
                        "item-edit",
                        "--project-id",
                        project_id,
                        "--id",
                        item_id,
                        "--field-id",
                        field_id,
                        "--single-select-option-id",
                        &option_id,
                    ],
                    workspace,
                )
                .await;

            match result {
                Ok(_) => {
                    return Ok(json!({ "updated": true }));
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < 2 {
                        tracing::warn!(
                            attempt,
                            item_id,
                            status,
                            "gh project item-edit failed, retrying"
                        );
                    }
                }
            }
        }

        let error = last_error
            .unwrap_or_else(|| AppError::new(ErrorCategory::ToolExecutionError, "unknown error"));

        if on_error == "warn" {
            tracing::warn!(
                item_id,
                status,
                error = %error.message,
                "project_item_set_status failed after retries"
            );
            return Ok(json!({
                "updated": false,
                "warning": error.message
            }));
        }

        Err(error)
    }
}
