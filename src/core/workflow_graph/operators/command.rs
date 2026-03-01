#![allow(clippy::result_large_err)] // Command operator returns AppError to surface shell execution diagnostics without boxing.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use serde_json::{Map, Number, Value};
use std::collections::HashMap;
use std::fs;
use std::iter::FromIterator;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::process::Command;
use tracing;

const OUTPUT_CAPTURE_LIMIT_BYTES: usize = 1_048_576;

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
        if !params.is_object() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "CommandOperator params must be an object",
            ));
        }
        let map = params.as_object().unwrap();
        if map
            .get("cmd")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
        {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "CommandOperator requires a non-empty cmd",
            ));
        }
        Ok(())
    }

    async fn execute(&self, params: Value, _ctx: ExecutionContext) -> Result<Value, AppError> {
        let parsed = CommandParams::from_value(&params)?;
        let resolved_cwd = parsed
            .cwd
            .as_deref()
            .map(|cwd| self.workspace_root.join(cwd))
            .unwrap_or_else(|| self.workspace_root.clone());

        tracing::debug!(
            cmd = %parsed.cmd,
            cwd = %resolved_cwd.display(),
            shell = parsed.shell,
            write_stdout = parsed.write_stdout.as_deref().unwrap_or("-"),
            write_stderr = parsed.write_stderr.as_deref().unwrap_or("-"),
            "executing command"
        );

        let start = Instant::now();
        let output = self
            .runner
            .run(&CommandExecutionRequest {
                cmd: parsed.cmd.clone(),
                cwd: resolved_cwd,
                env: parsed.env.clone(),
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
                        format!("failed to create directory for write_stdout: {}", err),
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
                        format!("failed to create directory for write_stderr: {}", err),
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

        if request.capture_stdout {
            command.stdout(Stdio::piped());
        } else {
            command.stdout(Stdio::inherit());
        }
        if request.capture_stderr {
            command.stderr(Stdio::piped());
        } else {
            command.stderr(Stdio::inherit());
        }

        command.stdin(Stdio::null());

        command.current_dir(request.cwd.clone());
        if let Some(env_map) = &request.env {
            command.envs(env_map);
        }

        let output = command.output().await.map_err(|err| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to execute command: {}", err),
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

struct CommandParams {
    cmd: String,
    cwd: Option<String>,
    env: Option<HashMap<String, String>>,
    capture_stdout: bool,
    capture_stderr: bool,
    shell: bool,
    write_stdout: Option<String>,
    write_stderr: Option<String>,
}

impl CommandParams {
    fn from_value(value: &Value) -> Result<Self, AppError> {
        let map = value.as_object().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ValidationError,
                "CommandOperator params must be an object",
            )
        })?;
        let cmd = map
            .get("cmd")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::new(ErrorCategory::ValidationError, "cmd is required"))?
            .to_string();

        let cwd = map
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::trim)
            .map(|s| s.to_string());
        if let Some(cwd_str) = &cwd {
            if Path::new(cwd_str).is_absolute() {
                return Err(
                    AppError::new(ErrorCategory::ValidationError, "cwd must be relative")
                        .with_code("WFG-CMD-001"),
                );
            }
        }

        let env = map.get("env").and_then(Value::as_object).map(|env_map| {
            env_map
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|v| (key.clone(), v.to_string())))
                .collect::<HashMap<_, _>>()
        });

        let capture_stdout = map
            .get("capture_stdout")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let capture_stderr = map
            .get("capture_stderr")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let shell = map.get("shell").and_then(Value::as_bool).unwrap_or(false);

        let write_stdout = map
            .get("write_stdout")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if let Some(ref p) = write_stdout {
            if Path::new(p).is_absolute() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "write_stdout must be relative",
                )
                .with_code("WFG-CMD-003"));
            }
        }

        let write_stderr = map
            .get("write_stderr")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if let Some(ref p) = write_stderr {
            if Path::new(p).is_absolute() {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "write_stderr must be relative",
                )
                .with_code("WFG-CMD-003"));
            }
        }

        Ok(Self {
            cmd,
            cwd,
            env,
            capture_stdout,
            capture_stderr,
            shell,
            write_stdout,
            write_stderr,
        })
    }
}

fn limit_bytes(bytes: &[u8]) -> String {
    let limit = OUTPUT_CAPTURE_LIMIT_BYTES.min(bytes.len());
    String::from_utf8_lossy(&bytes[..limit]).into_owned()
}
