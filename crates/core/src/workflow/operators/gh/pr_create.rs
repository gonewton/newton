use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use serde_json::{json, Map, Value};

use super::retry::RetryConfig;
use super::utils::extract_pr_number;
use super::GhOperator;

pub(super) fn validate_pr_create(map: &Map<String, Value>) -> Result<(), AppError> {
    if map
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "title is required for pr_create",
        ));
    }
    RetryConfig::validate(map)?;
    Ok(())
}

impl GhOperator {
    pub(super) async fn execute_pr_create(
        &self,
        map: &Map<String, Value>,
        workspace: &std::path::Path,
    ) -> Result<Value, AppError> {
        let base = map.get("base").and_then(Value::as_str).unwrap_or("main");
        let title = map.get("title").and_then(Value::as_str).unwrap();
        let body = map.get("body").and_then(Value::as_str).unwrap_or("");

        let config = RetryConfig::from_map(map);
        let mut delay_ms = config.start_delay_ms();
        let mut last_error: Option<AppError> = None;

        for attempt in 1..=config.count {
            let result = self
                .runner
                .run(
                    &[
                        "pr", "create", "--base", base, "--title", title, "--body", body,
                    ],
                    workspace,
                )
                .await;

            match result {
                Ok(output) => {
                    let pr_url = output.stdout.trim();
                    if pr_url.is_empty() {
                        last_error = Some(AppError::new(
                            ErrorCategory::ToolExecutionError,
                            "pr create returned empty URL",
                        ));
                    } else {
                        let pr_number = extract_pr_number(pr_url)?;
                        return Ok(json!({
                            "pr_url": pr_url,
                            "pr_number": pr_number
                        }));
                    }
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }

            config.backoff(attempt, &mut delay_ms, "pr create").await;
        }

        Err(last_error.unwrap_or_else(|| {
            AppError::new(ErrorCategory::ToolExecutionError, "pr create failed")
        }))
    }
}
