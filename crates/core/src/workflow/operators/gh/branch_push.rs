use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use serde_json::{json, Map, Value};

use super::retry::RetryConfig;
use super::GhOperator;

pub(super) fn validate_branch_push(map: &Map<String, Value>) -> Result<(), AppError> {
    if let Some(remote) = map.get("remote").and_then(Value::as_str) {
        let trimmed = remote.trim();
        if trimmed.is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "remote must not be empty after trimming",
            )
            .with_code("WFG-GH-009"));
        }
        if trimmed.chars().any(char::is_whitespace) {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "remote must not contain whitespace",
            )
            .with_code("WFG-GH-009"));
        }
        if trimmed.contains("..") {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "remote must not contain '..'",
            )
            .with_code("WFG-GH-009"));
        }
        if trimmed.starts_with('-') {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "remote must not start with '-'",
            )
            .with_code("WFG-GH-009"));
        }
    }

    if let Some(branch) = map.get("branch").and_then(Value::as_str) {
        if branch.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "branch must not be empty after trimming",
            )
            .with_code("WFG-GH-009"));
        }
    }

    if let Some(v) = map.get("set_upstream") {
        if v.as_bool().is_none() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "set_upstream must be a boolean",
            ));
        }
    }

    RetryConfig::validate(map)?;
    Ok(())
}

impl GhOperator {
    pub(super) async fn execute_branch_push(
        &self,
        map: &Map<String, Value>,
        workspace: &std::path::Path,
    ) -> Result<Value, AppError> {
        let remote = map
            .get("remote")
            .and_then(Value::as_str)
            .unwrap_or("origin");
        let branch = map.get("branch").and_then(Value::as_str).unwrap_or("HEAD");
        let set_upstream = map
            .get("set_upstream")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let config = RetryConfig::from_map(map);
        let mut delay_ms = config.start_delay_ms();

        let mut args: Vec<&str> = vec!["push"];
        if set_upstream {
            args.push("--set-upstream");
        }
        args.push(remote);
        args.push(branch);

        let mut last_error: Option<AppError> = None;

        for attempt in 1..=config.count {
            let result = self.git_runner.run(&args, workspace).await;

            match result {
                Ok(_output) => {
                    return Ok(json!({
                        "pushed": true,
                        "remote": remote,
                        "branch": branch,
                        "set_upstream": set_upstream,
                    }));
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }

            config.backoff(attempt, &mut delay_ms, "git push").await;
        }

        Err(last_error
            .unwrap_or_else(|| AppError::new(ErrorCategory::ToolExecutionError, "git push failed")))
    }
}
