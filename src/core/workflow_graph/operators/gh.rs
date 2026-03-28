#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;
use tracing;

const MAX_RETRY_DELAY_MS: u64 = 300_000;

pub struct GhOperator {
    runner: Arc<dyn GhRunner>,
}

impl GhOperator {
    pub fn new() -> Self {
        Self {
            runner: Arc::new(TokioGhRunner),
        }
    }

    pub fn with_runner(runner: Arc<dyn GhRunner>) -> Self {
        Self { runner }
    }
}

impl Default for GhOperator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Operator for GhOperator {
    fn name(&self) -> &'static str {
        "GhOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let map = params.as_object().ok_or_else(|| {
            AppError::new(ErrorCategory::ValidationError, "params must be an object")
        })?;

        let operation = map
            .get("operation")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::new(ErrorCategory::ValidationError, "operation is required")
            })?;

        match operation {
            "project_resolve_board" => {
                validate_project_resolve_board(map)?;
            }
            "project_item_set_status" => {
                validate_project_item_set_status(map)?;
            }
            "pr_create" => {
                validate_pr_create(map)?;
            }
            "pr_view" => {
                validate_pr_view(map)?;
            }
            _ => {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("unknown operation: {}", operation),
                ));
            }
        }

        Ok(())
    }

    async fn execute(&self, params: Value, _ctx: ExecutionContext) -> Result<Value, AppError> {
        let map = params.as_object().ok_or_else(|| {
            AppError::new(ErrorCategory::ValidationError, "params must be an object")
        })?;

        let operation = map
            .get("operation")
            .and_then(Value::as_str)
            .expect("operation validated");

        match operation {
            "project_resolve_board" => self.execute_project_resolve_board(map).await,
            "project_item_set_status" => self.execute_project_item_set_status(map).await,
            "pr_create" => self.execute_pr_create(map).await,
            "pr_view" => self.execute_pr_view(map).await,
            _ => Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("unknown operation: {}", operation),
            )),
        }
    }
}

fn validate_project_resolve_board(map: &Map<String, Value>) -> Result<(), AppError> {
    if map
        .get("owner")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "owner is required for project_resolve_board",
        ));
    }
    let project_number = map.get("project_number");
    match project_number {
        Some(Value::String(s)) if !s.is_empty() => {}
        Some(Value::Number(_)) => {}
        _ => {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "project_number is required for project_resolve_board",
            ));
        }
    }
    if let Some(arr) = map.get("required_option_names").and_then(Value::as_array) {
        if arr.is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "required_option_names must be a non-empty array when set",
            ));
        }
        for v in arr {
            if v.as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_none()
            {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "required_option_names must contain only non-empty strings",
                ));
            }
        }
    }
    Ok(())
}

fn validate_project_item_set_status(map: &Map<String, Value>) -> Result<(), AppError> {
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
    let status = map.get("status").and_then(Value::as_str).unwrap_or("");
    if !["Ready", "In progress", "In review", "Done", "Backlog"].contains(&status) {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "status must be one of: Ready, In progress, In review, Done, Backlog; got: {}",
                status
            ),
        ));
    }
    Ok(())
}

fn validate_pr_create(map: &Map<String, Value>) -> Result<(), AppError> {
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
    if let Some(retry_count) = map.get("retry_count").and_then(Value::as_i64) {
        if retry_count < 1 {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "retry_count must be at least 1",
            ));
        }
    }
    if let Some(delay) = map.get("retry_delay_ms").and_then(Value::as_i64) {
        if delay < 0 {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "retry_delay_ms must be non-negative",
            ));
        }
    }
    Ok(())
}

fn validate_pr_view(map: &Map<String, Value>) -> Result<(), AppError> {
    let pr = map.get("pr");
    match pr {
        Some(Value::String(s)) if !s.is_empty() => {}
        Some(Value::Number(_)) => {}
        _ => {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "pr is required for pr_view (string or number)",
            ));
        }
    }
    Ok(())
}

impl GhOperator {
    async fn execute_project_resolve_board(
        &self,
        map: &Map<String, Value>,
    ) -> Result<Value, AppError> {
        let owner = map.get("owner").and_then(Value::as_str).unwrap();
        let project_number = map
            .get("project_number")
            .map(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| v.to_string())
            })
            .unwrap();
        let field_name = map
            .get("field_name")
            .and_then(Value::as_str)
            .unwrap_or("Status");

        let view_output = self
            .runner
            .run(&[
                "project",
                "view",
                &project_number,
                "--owner",
                owner,
                "--format",
                "json",
            ])
            .await?;

        let view_json: Value = serde_json::from_str(&view_output.stdout).map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to parse project view JSON: {}", e),
            )
            .with_code("WFG-GH-001")
        })?;

        let project_id = view_json["id"].as_str().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                "project view missing id field",
            )
            .with_code("WFG-GH-001")
        })?;

        let fields_output = self
            .runner
            .run(&[
                "project",
                "field-list",
                &project_number,
                "--owner",
                owner,
                "--format",
                "json",
            ])
            .await?;

        let fields_json: Value = serde_json::from_str(&fields_output.stdout).map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to parse project field-list JSON: {}", e),
            )
            .with_code("WFG-GH-001")
        })?;

        let fields = fields_json["fields"].as_array().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                "field-list missing fields array",
            )
            .with_code("WFG-GH-001")
        })?;

        let field = fields
            .iter()
            .find(|f| f["name"].as_str() == Some(field_name))
            .ok_or_else(|| {
                AppError::new(
                    ErrorCategory::ToolExecutionError,
                    format!("field '{}' not found", field_name),
                )
                .with_code("WFG-GH-001")
            })?;

        let field_id = field["id"].as_str().ok_or_else(|| {
            AppError::new(ErrorCategory::ToolExecutionError, "field missing id")
                .with_code("WFG-GH-001")
        })?;

        let options = field["options"].as_array().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                "field missing options array",
            )
            .with_code("WFG-GH-001")
        })?;

        let default_required = vec![
            "Ready".to_string(),
            "In progress".to_string(),
            "In review".to_string(),
            "Done".to_string(),
        ];
        let required_names: Vec<String> = if let Some(arr) = map.get("required_option_names") {
            arr.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            default_required.clone()
        };
        let required_names = if required_names.is_empty() {
            default_required
        } else {
            required_names
        };

        let mut found_options: Vec<String> = Vec::new();
        let mut options_map: HashMap<String, String> = HashMap::new();

        for opt in options {
            if let (Some(name), Some(id)) = (opt["name"].as_str(), opt["id"].as_str()) {
                found_options.push(name.to_string());
                options_map.insert(name.to_string(), id.to_string());
            }
        }

        for required in &required_names {
            if !options_map.contains_key(required) {
                return Err(AppError::new(
                    ErrorCategory::ToolExecutionError,
                    format!(
                        "required option '{}' not found. Found options: {:?}",
                        required, found_options
                    ),
                )
                .with_code("WFG-GH-001"));
            }
        }

        let mut out = serde_json::Map::new();
        out.insert("project_id".to_string(), json!(project_id));
        out.insert("field_id".to_string(), json!(field_id));
        out.insert(
            "options".to_string(),
            Value::Object(
                options_map
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect(),
            ),
        );
        if let Some(id) = options_map.get("Ready") {
            out.insert("ready_id".to_string(), json!(id));
        }
        if let Some(id) = options_map.get("In progress") {
            out.insert("in_progress_id".to_string(), json!(id));
        }
        if let Some(id) = options_map.get("In review") {
            out.insert("in_review_id".to_string(), json!(id));
        }
        if let Some(id) = options_map.get("Done") {
            out.insert("done_id".to_string(), json!(id));
        }
        if let Some(id) = options_map.get("Backlog") {
            out.insert("backlog_id".to_string(), json!(id));
        }

        Ok(Value::Object(out))
    }

    async fn execute_project_item_set_status(
        &self,
        map: &Map<String, Value>,
    ) -> Result<Value, AppError> {
        let item_id = map.get("item_id").and_then(Value::as_str).unwrap();
        let board = map.get("board").and_then(Value::as_object).unwrap();
        let status = map.get("status").and_then(Value::as_str).unwrap();
        let on_error = map
            .get("on_error")
            .and_then(Value::as_str)
            .unwrap_or("warn");

        let option_id = resolve_option_id(board, status)?;

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
                .run(&[
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
                ])
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

    async fn execute_pr_create(&self, map: &Map<String, Value>) -> Result<Value, AppError> {
        let base = map.get("base").and_then(Value::as_str).unwrap_or("main");
        let title = map.get("title").and_then(Value::as_str).unwrap();
        let body = map.get("body").and_then(Value::as_str).unwrap_or("");
        let retry_count = map.get("retry_count").and_then(Value::as_i64).unwrap_or(3) as usize;
        let retry_delay_ms = map
            .get("retry_delay_ms")
            .and_then(Value::as_i64)
            .unwrap_or(5000) as u64;
        let capped_delay = retry_delay_ms.min(MAX_RETRY_DELAY_MS);

        let mut last_error: Option<AppError> = None;

        for attempt in 1..=retry_count {
            let result = self
                .runner
                .run(&[
                    "pr", "create", "--base", base, "--title", title, "--body", body,
                ])
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

            if attempt < retry_count {
                tracing::warn!(
                    attempt,
                    max_attempts = retry_count,
                    "pr create failed, retrying after delay"
                );
                sleep(Duration::from_millis(capped_delay)).await;
            }
        }

        Err(last_error.unwrap_or_else(|| {
            AppError::new(ErrorCategory::ToolExecutionError, "pr create failed")
        }))
    }

    async fn execute_pr_view(&self, map: &Map<String, Value>) -> Result<Value, AppError> {
        let pr = get_pr_identifier(map)?;

        let pr_number = pr.parse::<u64>().map_err(|_| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("pr must be a valid number, got: {}", pr),
            )
        })?;

        let output = self
            .runner
            .run(&["pr", "view", &pr, "--json", "state"])
            .await?;

        let pr_json: Value = serde_json::from_str(&output.stdout).map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to parse pr view JSON: {}", e),
            )
            .with_code("WFG-GH-002")
        })?;

        let state = pr_json["state"].as_str().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                "pr view missing state field",
            )
            .with_code("WFG-GH-002")
        })?;

        let normalized_state = state.to_uppercase();

        Ok(json!({
            "state": normalized_state,
            "pr_number": pr_number
        }))
    }
}

fn resolve_option_id(board: &Map<String, Value>, status: &str) -> Result<String, AppError> {
    if let Some(options) = board.get("options").and_then(Value::as_object) {
        if let Some(id) = options.get(status).and_then(Value::as_str) {
            return Ok(id.to_string());
        }
    }

    let flat_key = match status {
        "Ready" => "ready_id",
        "In progress" => "in_progress_id",
        "In review" => "in_review_id",
        "Done" => "done_id",
        "Backlog" => "backlog_id",
        _ => {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("unknown status: {}", status),
            ))
        }
    };

    board
        .get(flat_key)
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("option id for '{}' not found in board", status),
            )
        })
}

fn get_pr_identifier(map: &Map<String, Value>) -> Result<String, AppError> {
    let pr = map.get("pr").ok_or_else(|| {
        AppError::new(ErrorCategory::ValidationError, "pr is required for pr_view")
    })?;

    match pr {
        Value::String(s) => {
            if s.contains("/pull/") {
                if let Some(num) = s.rsplit('/').next() {
                    return Ok(num.to_string());
                }
            }
            Ok(s.clone())
        }
        Value::Number(n) => Ok(n.to_string()),
        _ => Err(AppError::new(
            ErrorCategory::ValidationError,
            "pr must be a string or number",
        )),
    }
}

fn extract_pr_number(url: &str) -> Result<u64, AppError> {
    let parts: Vec<&str> = url.rsplit('/').collect();
    parts
        .first()
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to extract PR number from: {}", url),
            )
            .with_code("WFG-GH-002")
        })
}

#[derive(Clone, Debug)]
pub struct GhOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[async_trait]
pub trait GhRunner: Send + Sync + 'static {
    async fn run(&self, args: &[&str]) -> Result<GhOutput, AppError>;
}

struct TokioGhRunner;

#[async_trait]
impl GhRunner for TokioGhRunner {
    async fn run(&self, args: &[&str]) -> Result<GhOutput, AppError> {
        let mut cmd = Command::new("gh");
        for arg in args {
            cmd.arg(arg);
        }
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        let output = cmd.output().await.map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to execute gh: {}", e),
            )
            .with_code("WFG-GH-003")
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let exit_code = output.status.code().unwrap_or(-1);

        if exit_code != 0 {
            return Err(AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("gh command failed with exit code {}: {}", exit_code, stderr),
            )
            .with_code("WFG-GH-004"));
        }

        Ok(GhOutput {
            stdout,
            stderr,
            exit_code,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_validate_project_resolve_board() {
        let params = json!({
            "operation": "project_resolve_board",
            "owner": "myorg",
            "project_number": 1
        });
        assert!(GhOperator::new().validate_params(&params).is_ok());

        let params_missing_owner = json!({
            "operation": "project_resolve_board",
            "project_number": 1
        });
        assert!(GhOperator::new()
            .validate_params(&params_missing_owner)
            .is_err());
    }

    #[test]
    fn test_validate_project_item_set_status() {
        let params = json!({
            "operation": "project_item_set_status",
            "item_id": "ITEM_123",
            "board": {"project_id": "P_123", "field_id": "F_123"},
            "status": "In progress"
        });
        assert!(GhOperator::new().validate_params(&params).is_ok());

        let params_invalid_status = json!({
            "operation": "project_item_set_status",
            "item_id": "ITEM_123",
            "board": {"project_id": "P_123", "field_id": "F_123"},
            "status": "Invalid"
        });
        assert!(GhOperator::new()
            .validate_params(&params_invalid_status)
            .is_err());

        let params_backlog = json!({
            "operation": "project_item_set_status",
            "item_id": "ITEM_123",
            "board": {"project_id": "P_123", "field_id": "F_123", "backlog_id": "OPT_b"},
            "status": "Backlog"
        });
        assert!(GhOperator::new().validate_params(&params_backlog).is_ok());
    }

    #[test]
    fn test_validate_pr_create() {
        let params = json!({
            "operation": "pr_create",
            "title": "My PR",
            "base": "main"
        });
        assert!(GhOperator::new().validate_params(&params).is_ok());

        let params_missing_title = json!({
            "operation": "pr_create",
            "base": "main"
        });
        assert!(GhOperator::new()
            .validate_params(&params_missing_title)
            .is_err());
    }

    #[test]
    fn test_validate_pr_view() {
        let params = json!({
            "operation": "pr_view",
            "pr": 123
        });
        assert!(GhOperator::new().validate_params(&params).is_ok());

        let params_with_url = json!({
            "operation": "pr_view",
            "pr": "https://github.com/owner/repo/pull/456"
        });
        assert!(GhOperator::new().validate_params(&params_with_url).is_ok());
    }

    #[test]
    fn test_resolve_option_id_from_options() {
        let board = json!({
            "project_id": "P_123",
            "field_id": "F_123",
            "options": {
                "Ready": "OPT_READY",
                "In progress": "OPT_IN_PROGRESS",
                "In review": "OPT_IN_REVIEW",
                "Done": "OPT_DONE"
            }
        });

        let map = board.as_object().unwrap();
        assert_eq!(resolve_option_id(map, "Ready").unwrap(), "OPT_READY");
        assert_eq!(
            resolve_option_id(map, "In progress").unwrap(),
            "OPT_IN_PROGRESS"
        );
    }

    #[test]
    fn test_resolve_option_id_from_flat() {
        let board = json!({
            "project_id": "P_123",
            "field_id": "F_123",
            "ready_id": "OPT_READY",
            "in_progress_id": "OPT_IN_PROGRESS",
            "in_review_id": "OPT_IN_REVIEW",
            "done_id": "OPT_DONE"
        });

        let map = board.as_object().unwrap();
        assert_eq!(resolve_option_id(map, "Ready").unwrap(), "OPT_READY");
        assert_eq!(
            resolve_option_id(map, "In progress").unwrap(),
            "OPT_IN_PROGRESS"
        );
    }

    #[test]
    fn test_extract_pr_number() {
        assert_eq!(
            extract_pr_number("https://github.com/owner/repo/pull/123").unwrap(),
            123
        );
        assert_eq!(
            extract_pr_number("https://github.com/owner/repo/pull/456").unwrap(),
            456
        );
        assert!(extract_pr_number("not-a-url").is_err());
    }

    #[test]
    fn test_get_pr_identifier() {
        let map = json!({"pr": 123}).as_object().unwrap().clone();
        assert_eq!(get_pr_identifier(&map).unwrap(), "123");

        let map = json!({"pr": "456"}).as_object().unwrap().clone();
        assert_eq!(get_pr_identifier(&map).unwrap(), "456");

        let map = json!({"pr": "https://github.com/owner/repo/pull/789"})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(get_pr_identifier(&map).unwrap(), "789");
    }
}
