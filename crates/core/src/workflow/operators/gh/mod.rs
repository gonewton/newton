#![allow(clippy::result_large_err)]

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
use retry::RetryConfig;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use utils::{
    extract_pr_number, get_pr_identifier, insert_status_ids, parse_pr_url, resolve_option_id,
    validate_repository_format,
};

pub use runners::{default_git_runner, default_runner, GhOutput, GhRunner, GitRunner};

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
    RetryConfig::validate(map)?;
    Ok(())
}

fn validate_branch_push(map: &Map<String, Value>) -> Result<(), AppError> {
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

fn validate_pr_approve(map: &Map<String, Value>) -> Result<(), AppError> {
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
    async fn execute_pr_approve(
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

    async fn execute_project_resolve_board(
        &self,
        map: &Map<String, Value>,
        workspace: &std::path::Path,
    ) -> Result<Value, AppError> {
        let owner = map.get("owner").and_then(Value::as_str).unwrap();
        let project_number = map
            .get("project_number")
            .map(|v| {
                v.as_str()
                    .map_or_else(|| v.to_string(), std::string::ToString::to_string)
            })
            .unwrap();
        let field_name = map
            .get("field_name")
            .and_then(Value::as_str)
            .unwrap_or("Status");

        let view_output = self
            .runner
            .run(
                &[
                    "project",
                    "view",
                    &project_number,
                    "--owner",
                    owner,
                    "--format",
                    "json",
                ],
                workspace,
            )
            .await?;

        let view_json: Value = serde_json::from_str(&view_output.stdout).map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to parse project view JSON: {e}"),
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
            .run(
                &[
                    "project",
                    "field-list",
                    &project_number,
                    "--owner",
                    owner,
                    "--format",
                    "json",
                ],
                workspace,
            )
            .await?;

        let fields_json: Value = serde_json::from_str(&fields_output.stdout).map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to parse project field-list JSON: {e}"),
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
                    format!("field '{field_name}' not found"),
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
        let required_names: Vec<String> = map
            .get("required_option_names")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| default_required.clone());

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
                        "required option '{required}' not found. Found options: {found_options:?}"
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
        insert_status_ids(&options_map, &mut out);

        Ok(Value::Object(out))
    }

    async fn execute_project_item_set_status(
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

    async fn execute_pr_create(
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

    async fn execute_pr_view(
        &self,
        map: &Map<String, Value>,
        workspace: &std::path::Path,
    ) -> Result<Value, AppError> {
        let pr = get_pr_identifier(map)?;

        let pr_number = pr.parse::<u64>().map_err(|_| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("pr must be a valid number, got: {pr}"),
            )
        })?;

        let output = self
            .runner
            .run(&["pr", "view", &pr, "--json", "state"], workspace)
            .await?;

        let pr_json: Value = serde_json::from_str(&output.stdout).map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to parse pr view JSON: {e}"),
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

    async fn execute_branch_push(
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
