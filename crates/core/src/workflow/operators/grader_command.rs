//! GraderCommandOperator — runs a shell command Grader, validates Assessment, persists, returns output.
//! Spec 062.

#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use crate::workflow::operators::assessment;
use async_trait::async_trait;
use chrono::Utc;
use newton_types::BackendStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use uuid::Uuid;

pub struct GraderCommandOperator {
    workspace_root: PathBuf,
    store: Arc<dyn BackendStore>,
}

impl GraderCommandOperator {
    pub fn new(workspace_root: PathBuf, store: Arc<dyn BackendStore>) -> Self {
        Self {
            workspace_root,
            store,
        }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct GraderCommandParams {
    /// Shell command to run (passed to shell -c cmd).
    pub cmd: String,
    /// Grader identifier (e.g. "test-coverage-grader").
    pub grader: String,
    /// Scope kind (e.g. "repo", "component").
    pub scope: String,
    /// Scope entity ID.
    pub scope_id: String,
    /// Shell to use (default: "bash").
    #[serde(default = "default_shell")]
    pub shell: String,
    /// Optional working directory (relative to workspace root).
    #[serde(default)]
    pub cwd: Option<String>,
    /// Timeout in seconds.
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    /// Additional environment variables.
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    /// State variables injected as NEWTON_STATE_<KEY> env vars.
    #[serde(default)]
    pub state: Option<HashMap<String, String>>,
}

fn default_shell() -> String {
    "bash".to_string()
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct GraderCommandOutput {
    pub overall_score: f64,
    pub verdict: String,
    pub score_by_dimension: Value,
    pub counts: Value,
    pub assessment: Value,
}

#[async_trait]
impl Operator for GraderCommandOperator {
    fn name(&self) -> &'static str {
        "GraderCommandOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let parsed: GraderCommandParams = serde_json::from_value(params.clone()).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("GraderCommandOperator params invalid: {e}"),
            )
            .with_code("GRADER-CMD-001")
        })?;
        if parsed.cmd.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "GraderCommandOperator requires a non-empty cmd",
            )
            .with_code("GRADER-CMD-001"));
        }
        if parsed.grader.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "GraderCommandOperator requires a non-empty grader",
            )
            .with_code("GRADER-CMD-001"));
        }
        Ok(())
    }

    fn params_schema(&self) -> schemars::Schema {
        schemars::schema_for!(GraderCommandParams)
    }

    fn output_schema(&self) -> schemars::Schema {
        schemars::schema_for!(GraderCommandOutput)
    }

    async fn execute(&self, params: Value, _ctx: ExecutionContext) -> Result<Value, AppError> {
        let parsed: GraderCommandParams = serde_json::from_value(params.clone()).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("GraderCommandOperator params invalid: {e}"),
            )
            .with_code("GRADER-CMD-001")
        })?;

        let resolved_cwd = parsed.cwd.as_deref().map_or_else(
            || self.workspace_root.clone(),
            |cwd| self.workspace_root.join(cwd),
        );

        // Build environment
        let mut env_map: HashMap<String, String> = parsed.env.clone().unwrap_or_default();
        env_map.insert(
            "NEWTON_WORKSPACE".to_string(),
            self.workspace_root.display().to_string(),
        );
        env_map.insert("NEWTON_SCOPE".to_string(), parsed.scope.clone());
        env_map.insert("NEWTON_SCOPE_ID".to_string(), parsed.scope_id.clone());
        env_map.insert("NEWTON_GRADER".to_string(), parsed.grader.clone());
        if let Some(state) = &parsed.state {
            for (k, v) in state {
                env_map.insert(format!("NEWTON_STATE_{}", k.to_uppercase()), v.clone());
            }
        }

        tracing::debug!(
            cmd = %parsed.cmd,
            grader = %parsed.grader,
            scope = %parsed.scope,
            scope_id = %parsed.scope_id,
            cwd = %resolved_cwd.display(),
            "GraderCommandOperator: running grader command"
        );

        // Spawn command
        let mut cmd = Command::new(&parsed.shell);
        cmd.arg("-c").arg(&parsed.cmd);
        cmd.current_dir(&resolved_cwd);
        cmd.envs(&env_map);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.stdin(Stdio::null());

        // M2: enforce timeout_seconds (default 120 s).
        let timeout_dur = std::time::Duration::from_secs(parsed.timeout_seconds.unwrap_or(120));
        let output = tokio::time::timeout(timeout_dur, cmd.output())
            .await
            .map_err(|_| {
                AppError::new(
                    ErrorCategory::ToolExecutionError,
                    format!(
                        "GraderCommandOperator: command timed out after {}s",
                        parsed.timeout_seconds.unwrap_or(120)
                    ),
                )
                .with_code("GRADER-CMD-005")
            })?
            .map_err(|e| {
                AppError::new(
                    ErrorCategory::ToolExecutionError,
                    format!("GraderCommandOperator: failed to spawn command: {e}"),
                )
                .with_code("GRADER-CMD-002")
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        if !output.status.success() {
            let exit_code = output.status.code().unwrap_or(-1);
            return Err(AppError::new(
                ErrorCategory::ToolExecutionError,
                format!(
                    "GraderCommandOperator: command exited with code {exit_code}. stderr: {stderr}"
                ),
            )
            .with_code("GRADER-CMD-003"));
        }

        // Parse stdout as JSON Assessment
        let mut assessment_json: Value = serde_json::from_str(&stdout).map_err(|e| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("GraderCommandOperator: stdout is not valid JSON: {e}. stdout: {stdout}"),
            )
            .with_code("GRADER-CMD-004")
        })?;

        // M1: overwrite envelope fields authoritatively (operator owns these).
        let now = Utc::now().to_rfc3339();
        if let Some(obj) = assessment_json.as_object_mut() {
            obj.insert("grader".to_string(), Value::String(parsed.grader.clone()));
            obj.insert("scope".to_string(), Value::String(parsed.scope.clone()));
            obj.insert(
                "scope_id".to_string(),
                Value::String(parsed.scope_id.clone()),
            );
            obj.insert("evaluated_at".to_string(), Value::String(now.clone()));
        }

        // Validate assessment schema
        let content = assessment::validate_assessment(&assessment_json)?;

        // Persist assessment
        let run_id = Uuid::new_v4().to_string();
        assessment::persist_assessment(
            &self.store,
            &run_id,
            &parsed.grader,
            &parsed.scope,
            &parsed.scope_id,
            &content,
            &assessment_json,
            &now,
        )
        .await?;

        // Return structured output
        Ok(assessment::build_output(&content, assessment_json))
    }
}
