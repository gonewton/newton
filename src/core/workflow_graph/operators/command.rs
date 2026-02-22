use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use serde_json::{Map, Number, Value};
use std::collections::HashMap;
use std::iter::FromIterator;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;
use tokio::process::Command;

const OUTPUT_CAPTURE_LIMIT_BYTES: usize = 1_048_576;

pub struct CommandOperator {
    workspace_root: PathBuf,
}

impl CommandOperator {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
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

        let mut command = if parsed.shell {
            let mut cmd = Command::new("bash");
            cmd.arg("-lc").arg(parsed.cmd.clone());
            cmd
        } else {
            let mut parts = parsed.cmd.split_whitespace();
            let program = parts.next().ok_or_else(|| {
                AppError::new(ErrorCategory::ValidationError, "cmd string is empty")
            })?;
            let mut cmd = Command::new(program);
            for arg in parts {
                cmd.arg(arg);
            }
            cmd
        };

        if parsed.capture_stdout {
            command.stdout(Stdio::piped());
        } else {
            command.stdout(Stdio::null());
        }
        if parsed.capture_stderr {
            command.stderr(Stdio::piped());
        } else {
            command.stderr(Stdio::null());
        }

        command.current_dir(resolved_cwd);
        if let Some(env_map) = parsed.env {
            command.envs(env_map);
        }

        let start = Instant::now();
        let output = command.output().await.map_err(|err| {
            AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("failed to execute command: {}", err),
            )
            .with_code("WFG-CMD-002")
        })?;
        let duration_ms = start.elapsed().as_millis() as u64;

        let stdout = limit_bytes(&output.stdout);
        let stderr = limit_bytes(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        Ok(Value::Object(Map::from_iter([
            (
                "exit_code".to_string(),
                Value::Number(Number::from(exit_code)),
            ),
            ("stdout".to_string(), Value::String(stdout)),
            ("stderr".to_string(), Value::String(stderr)),
            (
                "duration_ms".to_string(),
                Value::Number(Number::from(duration_ms)),
            ),
        ])))
    }
}

struct CommandParams {
    cmd: String,
    cwd: Option<String>,
    env: Option<HashMap<String, String>>,
    capture_stdout: bool,
    capture_stderr: bool,
    shell: bool,
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

        Ok(Self {
            cmd,
            cwd,
            env,
            capture_stdout,
            capture_stderr,
            shell,
        })
    }
}

fn limit_bytes(bytes: &[u8]) -> String {
    let limit = OUTPUT_CAPTURE_LIMIT_BYTES.min(bytes.len());
    String::from_utf8_lossy(&bytes[..limit]).into_owned()
}
