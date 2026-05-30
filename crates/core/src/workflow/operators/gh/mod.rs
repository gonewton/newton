#![allow(clippy::result_large_err)]

mod branch_push;
mod pr_approve;
mod pr_create;
mod pr_view;
mod project_board;
mod project_status;
mod retry;
mod runners;
mod utils;

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use crate::workflow::operators::gh_authorization::{
    parse_authorization_params, AiloopApprover, ApprovalOutcome, AuthorizationParams,
    AuthorizationRequest, NoopApprover, OnUnavailable,
};
use async_trait::async_trait;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;

#[cfg(test)]
use serde_json::json;
#[cfg(test)]
use utils::{extract_pr_number, get_pr_identifier, resolve_option_id};

pub use runners::{default_git_runner, default_runner, GhOutput, GhRunner, GitRunner};

use branch_push::validate_branch_push;
use pr_approve::validate_pr_approve;
use pr_create::validate_pr_create;
use project_board::validate_project_resolve_board;
use project_status::validate_project_item_set_status;

const DEFAULT_AUTH_TIMEOUT: Duration = Duration::from_secs(300);

pub struct GhOperator {
    runner: Arc<dyn GhRunner>,
    approver: Arc<dyn AiloopApprover>,
    git_runner: Arc<dyn GitRunner>,
}

impl GhOperator {
    pub fn new() -> Self {
        Self {
            runner: Arc::new(default_runner()),
            approver: Arc::new(NoopApprover),
            git_runner: Arc::new(default_git_runner()),
        }
    }

    pub fn with_runner(runner: Arc<dyn GhRunner>) -> Self {
        Self {
            runner,
            approver: Arc::new(NoopApprover),
            git_runner: Arc::new(default_git_runner()),
        }
    }

    pub fn with_runner_and_approver(
        runner: Arc<dyn GhRunner>,
        approver: Arc<dyn AiloopApprover>,
    ) -> Self {
        Self {
            runner,
            approver,
            git_runner: Arc::new(default_git_runner()),
        }
    }

    pub fn with_all(
        runner: Arc<dyn GhRunner>,
        git_runner: Arc<dyn GitRunner>,
        approver: Arc<dyn AiloopApprover>,
    ) -> Self {
        Self {
            runner,
            approver,
            git_runner,
        }
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
            "project_resolve_board" => validate_project_resolve_board(map)?,
            "project_item_set_status" => validate_project_item_set_status(map)?,
            "pr_create" => {
                validate_pr_create(map)?;
            }
            "pr_view" => {}
            "pr_approve" => {
                validate_pr_approve(map)?;
            }
            "branch_push" => {
                validate_branch_push(map)?;
            }
            _ => {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("unknown operation: {operation}"),
                ));
            }
        }

        parse_authorization_params(map)?;
        Ok(())
    }

    async fn execute(&self, params: Value, ctx: ExecutionContext) -> Result<Value, AppError> {
        let map = params.as_object().ok_or_else(|| {
            AppError::new(ErrorCategory::ValidationError, "params must be an object")
        })?;

        let operation = map
            .get("operation")
            .and_then(Value::as_str)
            .expect("operation validated");

        let auth = parse_authorization_params(map)?;
        if auth.require {
            self.gate_authorization(&auth, operation, map, &ctx).await?;
        }

        match operation {
            "project_resolve_board" => {
                self.execute_project_resolve_board(map, &ctx.workspace_path)
                    .await
            }
            "project_item_set_status" => {
                self.execute_project_item_set_status(map, &ctx.workspace_path)
                    .await
            }
            "pr_create" => self.execute_pr_create(map, &ctx.workspace_path).await,
            "pr_view" => self.execute_pr_view(map, &ctx.workspace_path).await,
            "pr_approve" => self.execute_pr_approve(map, &ctx.workspace_path).await,
            "branch_push" => self.execute_branch_push(map, &ctx.workspace_path).await,
            _ => Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("unknown operation: {operation}"),
            )),
        }
    }
}

impl GhOperator {
    async fn gate_authorization(
        &self,
        auth: &AuthorizationParams,
        operation: &str,
        map: &Map<String, Value>,
        ctx: &ExecutionContext,
    ) -> Result<(), AppError> {
        let prompt = auth
            .prompt
            .clone()
            .unwrap_or_else(|| derive_default_prompt(operation, map));
        let timeout = auth.timeout.or(Some(DEFAULT_AUTH_TIMEOUT));
        let request_id = build_request_id(operation, &ctx.task_id, map);
        let request = AuthorizationRequest {
            request_id,
            prompt,
            channel: auth.channel.clone(),
            timeout,
            operation: operation.to_string(),
            task_id: Some(ctx.task_id.clone()),
        };

        let span = tracing::info_span!(
            "gh_authorization",
            task_id = %ctx.task_id,
            operation = operation
        );
        let _enter = span.enter();

        let outcome = self.approver.authorize(request).await?;
        match outcome {
            ApprovalOutcome::Approved => Ok(()),
            ApprovalOutcome::Denied { reason } => {
                let msg = match reason {
                    Some(r) => format!("authorization denied: {r}"),
                    None => "authorization denied".to_string(),
                };
                Err(AppError::new(ErrorCategory::ValidationError, msg).with_code("WFG-GH-AUTH-001"))
            }
            ApprovalOutcome::Timeout => Err(AppError::new(
                ErrorCategory::TimeoutError,
                "authorization request timed out",
            )
            .with_code("WFG-GH-AUTH-002")),
            ApprovalOutcome::Unavailable { cause } => match auth.on_unavailable {
                OnUnavailable::Skip => {
                    tracing::warn!(
                        task_id = %ctx.task_id,
                        operation = operation,
                        cause = %cause,
                        "ailoop approver unavailable; skipping authorization (on_authorization_unavailable=skip)"
                    );
                    Ok(())
                }
                OnUnavailable::Fail => Err(AppError::new(
                    ErrorCategory::ToolExecutionError,
                    format!("authorization unavailable: {cause}"),
                )
                .with_code("WFG-GH-AUTH-003")),
            },
        }
    }
}

fn derive_default_prompt(operation: &str, map: &Map<String, Value>) -> String {
    match operation {
        "pr_create" => {
            let title = map.get("title").and_then(Value::as_str).unwrap_or("");
            let base = map.get("base").and_then(Value::as_str).unwrap_or("main");
            format!("Authorize gh pr create: title=\"{title}\", base=\"{base}\"")
        }
        "pr_view" => {
            let pr = map
                .get("pr")
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => String::new(),
                })
                .unwrap_or_default();
            format!("Authorize gh pr view: pr={pr}")
        }
        "project_resolve_board" => {
            let owner = map.get("owner").and_then(Value::as_str).unwrap_or("");
            let project = map
                .get("project_number")
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => String::new(),
                })
                .unwrap_or_default();
            format!("Authorize gh project view/field-list: owner={owner}, project={project}")
        }
        "project_item_set_status" => {
            let item = map.get("item_id").and_then(Value::as_str).unwrap_or("");
            if let Some(oid) = map
                .get("single_select_option_id")
                .or_else(|| map.get("option_id"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                format!(
                    "Authorize gh project item-edit: item={item}, single_select_option_id={oid}"
                )
            } else {
                let status = map.get("status").and_then(Value::as_str).unwrap_or("");
                format!("Authorize gh project item-edit: item={item}, status={status}")
            }
        }
        "pr_approve" => {
            let selector = map
                .get("pr_url")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    map.get("pr_number")
                        .and_then(Value::as_i64)
                        .map(|n| n.to_string())
                })
                .unwrap_or_default();
            let repo = map.get("repository").and_then(Value::as_str).unwrap_or("");
            if repo.is_empty() {
                format!("Authorize gh pr review --approve: pr={selector}")
            } else {
                format!("Authorize gh pr review --approve: pr={selector}, repository={repo}")
            }
        }
        "branch_push" => {
            let remote = map
                .get("remote")
                .and_then(Value::as_str)
                .unwrap_or("origin");
            let branch = map.get("branch").and_then(Value::as_str).unwrap_or("HEAD");
            format!("Authorize git push: remote={remote}, branch={branch}")
        }
        _ => format!("Authorize gh {operation}"),
    }
}

fn build_request_id(operation: &str, task_id: &str, map: &Map<String, Value>) -> String {
    let payload = serde_json::to_string(map).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    let digest = hasher.finalize();
    let short: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("gh:{operation}:{task_id}:{short}")
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let params_custom_status_ok = json!({
            "operation": "project_item_set_status",
            "item_id": "ITEM_123",
            "board": {"project_id": "P_123", "field_id": "F_123"},
            "status": "Custom stage"
        });
        assert!(GhOperator::new()
            .validate_params(&params_custom_status_ok)
            .is_ok());

        let params_explicit_id = json!({
            "operation": "project_item_set_status",
            "item_id": "ITEM_123",
            "board": {"project_id": "P_123", "field_id": "F_123"},
            "single_select_option_id": "OPT_x"
        });
        assert!(GhOperator::new()
            .validate_params(&params_explicit_id)
            .is_ok());

        let params_neither = json!({
            "operation": "project_item_set_status",
            "item_id": "ITEM_123",
            "board": {"project_id": "P_123", "field_id": "F_123"},
        });
        assert!(GhOperator::new().validate_params(&params_neither).is_err());

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
                "Done": "OPT_DONE",
                "Custom stage": "OPT_CUSTOM"
            }
        });

        let map = board.as_object().unwrap();
        assert_eq!(resolve_option_id(map, "Ready").unwrap(), "OPT_READY");
        assert_eq!(
            resolve_option_id(map, "In progress").unwrap(),
            "OPT_IN_PROGRESS"
        );
        assert_eq!(
            resolve_option_id(map, "Custom stage").unwrap(),
            "OPT_CUSTOM"
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
