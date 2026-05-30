use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use serde_json::{json, Map, Value};

use super::utils::{parse_pr_url, validate_repository_format};
use super::GhOperator;

pub(super) fn validate_pr_approve(map: &Map<String, Value>) -> Result<(), AppError> {
    let has_number = map.get("pr_number").is_some();
    let has_url = map.get("pr_url").is_some();
    if has_number == has_url {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "pr_approve requires exactly one of pr_number or pr_url",
        )
        .with_code("WFG-GH-005"));
    }
    if has_number {
        let n = map
            .get("pr_number")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    "pr_number must be an integer",
                )
                .with_code("WFG-GH-008")
            })?;
        if n < 1 {
            return Err(
                AppError::new(ErrorCategory::ValidationError, "pr_number must be >= 1")
                    .with_code("WFG-GH-008"),
            );
        }
        if let Some(repo) = map.get("repository").and_then(Value::as_str) {
            validate_repository_format(repo)?;
        }
    } else {
        let url = map.get("pr_url").and_then(Value::as_str).ok_or_else(|| {
            AppError::new(ErrorCategory::ValidationError, "pr_url must be a string")
                .with_code("WFG-GH-006")
        })?;
        parse_pr_url(url)?;
    }
    Ok(())
}

impl GhOperator {
    pub(super) async fn execute_pr_approve(
        &self,
        map: &Map<String, Value>,
        workspace: &std::path::Path,
    ) -> Result<Value, AppError> {
        let (pr_number, repository, pr_url_input) =
            if let Some(url) = map.get("pr_url").and_then(Value::as_str) {
                let (owner_repo, number) = parse_pr_url(url)?;
                (number, Some(owner_repo), Some(url.to_string()))
            } else {
                let n = map.get("pr_number").and_then(Value::as_i64).unwrap() as u64;
                let repo = map
                    .get("repository")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                (n, repo, None)
            };

        let pr_str = pr_number.to_string();
        let mut args: Vec<&str> = vec!["pr", "review", &pr_str, "--approve"];
        if let Some(ref repo) = repository {
            args.push("-R");
            args.push(repo);
        }

        self.runner.run(&args, workspace).await?;

        let mut out = serde_json::Map::new();
        out.insert("review_submitted".to_string(), json!(true));
        out.insert("pr_number".to_string(), json!(pr_number));
        if let Some(ref repo) = repository {
            out.insert("repository".to_string(), json!(repo));
        }
        if let Some(url) = pr_url_input {
            out.insert("pr_url".to_string(), json!(url));
        } else if let Some(ref repo) = repository {
            out.insert(
                "pr_url".to_string(),
                json!(format!("https://github.com/{repo}/pull/{pr_number}")),
            );
        }

        Ok(Value::Object(out))
    }
}
