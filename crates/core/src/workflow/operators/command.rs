#![allow(clippy::result_large_err)] // Command operator returns AppError to surface shell execution diagnostics without boxing.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use crate::workflow::operators::OUTPUT_CAPTURE_LIMIT_BYTES;
use crate::workflow::subprocess::run_guarded;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::collections::HashMap;
use std::fs;
use std::iter::FromIterator;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::process::Command;
use tracing;

pub struct CommandOperator {
    workspace_root: PathBuf,
    runner: Arc<dyn CommandRunner>,
}

impl CommandOperator {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            runner: Arc::new(TokioCommandRunner),
        }
    }

    pub fn with_runner(workspace_root: PathBuf, runner: Arc<dyn CommandRunner>) -> Self {
        Self {
            workspace_root,
            runner,
        }
    }
}

#[async_trait]
impl Operator for CommandOperator {
    fn name(&self) -> &'static str {
        "CommandOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let parsed: CommandParams = serde_json::from_value(params.clone()).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("CommandOperator params invalid: {e}"),
            )
        })?;
        if parsed.cmd.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "CommandOperator requires a non-empty cmd",
            ));
        }
        if let Some(cwd_str) = &parsed.cwd {
            if Path::new(cwd_str).is_absolute() {
                return Err(
                    AppError::new(ErrorCategory::ValidationError, "cwd must be relative")
                        .with_code("WFG-CMD-001"),
                );
            }
        }
        if let Some(ref p) = parsed.write_stdout {
            if Path::new(p).is_absolute() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "write_stdout must be relative",
                )
                .with_code("WFG-CMD-003"));
            }
        }
        if let Some(ref p) = parsed.write_stderr {
            if Path::new(p).is_absolute() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "write_stderr must be relative",
                )
                .with_code("WFG-CMD-003"));
            }
        }
        Ok(())
    }

    fn params_schema(&self) -> schemars::Schema {
        schemars::schema_for!(CommandParams)
    }

    fn output_schema(&self) -> schemars::Schema {
        schemars::schema_for!(CommandOutput)
    }

    async fn execute(&self, params: Value, ctx: ExecutionContext) -> Result<Value, AppError> {
        let parsed: CommandParams = serde_json::from_value(params.clone()).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("CommandOperator params invalid: {e}"),
            )
        })?;
        let resolved_cwd = parsed.cwd.as_deref().map_or_else(
            || self.workspace_root.clone(),
            |cwd| self.workspace_root.join(cwd),
        );

        tracing::debug!(
            cmd = %parsed.cmd,
            cwd = %resolved_cwd.display(),
            shell = parsed.shell,
            write_stdout = parsed.write_stdout.as_deref().unwrap_or("-"),
            write_stderr = parsed.write_stderr.as_deref().unwrap_or("-"),
            "executing command"
        );

        // Start from the resolved state root (if any) so child `newton`
        // invocations shelled out by this command resolve the same state
        // root as the in-process executor (spec 074 decision 2). Explicit
        // `env` set in the workflow YAML always wins, so overlay it second.
        let env = match (&ctx.execution_overrides.state_dir, &parsed.env) {
            (None, None) => None,
            (state_dir, explicit) => {
                let mut merged = HashMap::new();
                if let Some(state_dir) = state_dir {
                    merged.insert(
                        "NEWTON_STATE_DIR".to_string(),
                        state_dir.display().to_string(),
                    );
                }
                if let Some(explicit) = explicit {
                    merged.extend(explicit.clone());
                }
                Some(merged)
            }
        };

        let start = Instant::now();
        let output = self
            .runner
            .run(&CommandExecutionRequest {
                cmd: parsed.cmd.clone(),
                cwd: resolved_cwd,
                env,
                capture_stdout: parsed.capture_stdout,
                capture_stderr: parsed.capture_stderr,
                shell: parsed.shell,
            })
            .await?;
        let duration_ms = start.elapsed().as_millis() as u64;

        let stdout = limit_bytes(&output.stdout);
        let stderr = limit_bytes(&output.stderr);

        if let Some(ref rel_path) = parsed.write_stdout {
            let abs_path = self.workspace_root.join(rel_path);
            if let Some(parent) = abs_path.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!("failed to create directory for write_stdout: {err}"),
                    )
                    .with_code("WFG-CMD-004")
                })?;
            }
            fs::write(&abs_path, stdout.as_bytes()).map_err(|err| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to write stdout to {}: {}", abs_path.display(), err),
                )
                .with_code("WFG-CMD-004")
            })?;
        }

        if let Some(ref rel_path) = parsed.write_stderr {
            let abs_path = self.workspace_root.join(rel_path);
            if let Some(parent) = abs_path.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!("failed to create directory for write_stderr: {err}"),
                    )
                    .with_code("WFG-CMD-004")
                })?;
            }
            fs::write(&abs_path, stderr.as_bytes()).map_err(|err| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to write stderr to {}: {}", abs_path.display(), err),
                )
                .with_code("WFG-CMD-004")
            })?;
        }

        let value = Value::Object(Map::from_iter([
            (
                "exit_code".to_string(),
                Value::Number(Number::from(output.exit_code)),
            ),
            ("stdout".to_string(), Value::String(stdout)),
            ("stderr".to_string(), Value::String(stderr)),
            (
                "duration_ms".to_string(),
                Value::Number(Number::from(duration_ms)),
            ),
            ("success".to_string(), Value::Bool(output.exit_code == 0)),
        ]));

        if output.exit_code != 0 {
            let mut err = AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("command failed with exit code {}", output.exit_code),
            )
            .with_code("WFG-CMD-001");
            err.add_context("output", &serde_json::to_string(&value).unwrap_or_default());
            return Err(err);
        }

        Ok(value)
    }
}

#[derive(Clone, Debug)]
pub struct CommandExecutionRequest {
    pub cmd: String,
    pub cwd: PathBuf,
    pub env: Option<HashMap<String, String>>,
    pub capture_stdout: bool,
    pub capture_stderr: bool,
    pub shell: bool,
}

#[derive(Clone, Debug)]
pub struct CommandExecutionOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

#[async_trait]
pub trait CommandRunner: Send + Sync + 'static {
    async fn run(
        &self,
        request: &CommandExecutionRequest,
    ) -> Result<CommandExecutionOutput, AppError>;
}

struct TokioCommandRunner;

#[async_trait]
impl CommandRunner for TokioCommandRunner {
    async fn run(
        &self,
        request: &CommandExecutionRequest,
    ) -> Result<CommandExecutionOutput, AppError> {
        let mut command = if request.shell {
            let mut cmd = Command::new("bash");
            cmd.arg("-lc").arg(request.cmd.clone());
            cmd
        } else {
            let mut parts = request.cmd.split_whitespace();
            let program = parts.next().ok_or_else(|| {
                AppError::new(ErrorCategory::ValidationError, "cmd string is empty")
            })?;
            let mut cmd = Command::new(program);
            for arg in parts {
                cmd.arg(arg);
            }
            cmd
        };

        // Stdio is intentionally not configured here: `run_guarded` forces
        // stdout/stderr to `Stdio::piped()` and stdin to `Stdio::null()`
        // unconditionally, mirroring `Command::output()`'s contract (see
        // its doc comment). `capture_stdout`/`capture_stderr` never
        // controlled stdio wiring in that contract — `Command::output()`
        // always captures both — so they carry no runtime behavior here;
        // they remain on `CommandParams`/`CommandExecutionRequest` as
        // documented (if inert) parts of the operator's public schema.
        command.current_dir(request.cwd.clone());
        if let Some(env_map) = &request.env {
            command.envs(env_map);
        }

        // See `workflow::subprocess::run_guarded`: group-wide kill guard so
        // an outer task timeout dropping this future can't orphan a
        // grandchild the shelled-out command spawns.
        let output = run_guarded(command).await.map_err(|err| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to execute command: {err}"),
            )
            .with_code("WFG-CMD-002")
        })?;

        Ok(CommandExecutionOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

fn default_capture_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandParams {
    pub cmd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(default = "default_capture_true")]
    pub capture_stdout: bool,
    #[serde(default = "default_capture_true")]
    pub capture_stderr: bool,
    #[serde(default)]
    pub shell: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_stdout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_stderr: Option<String>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub success: bool,
    pub duration_ms: u64,
}

fn limit_bytes(bytes: &[u8]) -> String {
    let limit = OUTPUT_CAPTURE_LIMIT_BYTES.min(bytes.len());
    String::from_utf8_lossy(&bytes[..limit]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::executor::GraphHandle;
    use crate::workflow::operator::{OperatorRegistry, StateView};
    use serde_json::json;
    use tempfile::TempDir;

    fn make_ctx(state_dir: Option<PathBuf>, workspace: &TempDir) -> ExecutionContext {
        ExecutionContext {
            workspace_path: workspace.path().to_path_buf(),
            execution_id: "test-exec-cmd-001".to_string(),
            task_id: "cmd".to_string(),
            iteration: 1,
            state_view: StateView::new(json!({}), json!({}), json!({})),
            graph: GraphHandle::new(HashMap::new()),
            workflow_file: workspace.path().join("workflow.yaml"),
            nesting_depth: 0,
            execution_overrides: crate::workflow::executor::ExecutionOverrides {
                parallel_limit: None,
                max_time_seconds: None,
                checkpoint_base_path: None,
                artifact_base_path: None,
                max_nesting_depth: None,
                verbose: false,
                sink: None,
                pre_seed_nodes: true,
                state_dir,
            },
            operator_registry: OperatorRegistry::new(),
        }
    }

    // ── Fix 2: NEWTON_STATE_DIR propagation to CommandOperator subprocesses ──

    #[tokio::test]
    async fn execute_injects_newton_state_dir_from_overrides() {
        let workspace = TempDir::new().unwrap();
        let state_dir = TempDir::new().unwrap();
        let op = CommandOperator::new(workspace.path().to_path_buf());
        let ctx = make_ctx(Some(state_dir.path().to_path_buf()), &workspace);
        let params = json!({
            "cmd": "printf '%s' \"$NEWTON_STATE_DIR\"",
            "shell": true,
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(
            result["stdout"],
            json!(state_dir.path().display().to_string())
        );
    }

    #[tokio::test]
    async fn execute_explicit_env_wins_over_newton_state_dir_override() {
        let workspace = TempDir::new().unwrap();
        let state_dir = TempDir::new().unwrap();
        let op = CommandOperator::new(workspace.path().to_path_buf());
        let ctx = make_ctx(Some(state_dir.path().to_path_buf()), &workspace);
        let params = json!({
            "cmd": "printf '%s' \"$NEWTON_STATE_DIR\"",
            "shell": true,
            "env": { "NEWTON_STATE_DIR": "/explicit" }
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["stdout"], json!("/explicit"));
    }

    #[tokio::test]
    async fn execute_no_overrides_state_dir_leaves_var_absent() {
        let workspace = TempDir::new().unwrap();
        let op = CommandOperator::new(workspace.path().to_path_buf());
        let ctx = make_ctx(None, &workspace);
        let params = json!({
            "cmd": "printf '%s' \"${NEWTON_STATE_DIR:-unset}\"",
            "shell": true,
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(result["stdout"], json!("unset"));
    }

    // ── Fix 1: run_guarded must mirror Command::output()'s forced-pipe
    // semantics, so capture_stdout:false does not leak the child's stdout
    // onto newton's own fd1 nor return an empty `output.stdout` ──

    #[tokio::test]
    async fn execute_with_capture_stdout_false_still_returns_stdout_content() {
        let workspace = TempDir::new().unwrap();
        let op = CommandOperator::new(workspace.path().to_path_buf());
        let ctx = make_ctx(None, &workspace);
        let params = json!({
            "cmd": "echo capture-stdout-false-marker",
            "shell": true,
            "capture_stdout": false,
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(
            result["stdout"],
            json!("capture-stdout-false-marker\n"),
            "capture_stdout:false must still return the child's stdout in output.stdout \
             (matches the pre-regression Command::output() contract, which always pipes \
             both streams); got {result}"
        );
    }

    #[tokio::test]
    async fn execute_with_capture_stderr_false_still_returns_stderr_content() {
        let workspace = TempDir::new().unwrap();
        let op = CommandOperator::new(workspace.path().to_path_buf());
        let ctx = make_ctx(None, &workspace);
        let params = json!({
            "cmd": "echo capture-stderr-false-marker >&2",
            "shell": true,
            "capture_stderr": false,
        });
        let result = op.execute(params, ctx).await.unwrap();
        assert_eq!(
            result["stderr"],
            json!("capture-stderr-false-marker\n"),
            "capture_stderr:false must still return the child's stderr in output.stderr; \
             got {result}"
        );
    }
}
