#![allow(clippy::unnecessary_cast)]

use crate::core::entities::ToolType;
use crate::core::entities::{ExecutionConfiguration, ToolMetadata};
use crate::core::error::AppError;
use crate::tools::ToolResult;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use uuid::Uuid;

pub struct ToolExecutor;

struct ToolExecutionEnv {
    vars: HashMap<String, String>,
}

impl ToolExecutionEnv {
    fn from_configuration(configuration: &ExecutionConfiguration, workspace_path: &Path) -> Self {
        let mut vars = HashMap::new();
        if let Some(eval_cmd) = &configuration.evaluator_cmd {
            vars.insert("NEWTON_EVALUATOR_CMD".to_string(), eval_cmd.clone());
        }
        if let Some(adv_cmd) = &configuration.advisor_cmd {
            vars.insert("NEWTON_ADVISOR_CMD".to_string(), adv_cmd.clone());
        }
        if let Some(exec_cmd) = &configuration.executor_cmd {
            vars.insert("NEWTON_EXECUTOR_CMD".to_string(), exec_cmd.clone());
        }
        if let Some(eval_timeout) = configuration.evaluator_timeout_ms {
            vars.insert(
                "NEWTON_EVALUATOR_TIMEOUT_MS".to_string(),
                eval_timeout.to_string(),
            );
        }
        if let Some(adv_timeout) = configuration.advisor_timeout_ms {
            vars.insert(
                "NEWTON_ADVISOR_TIMEOUT_MS".to_string(),
                adv_timeout.to_string(),
            );
        }
        if let Some(exec_timeout) = configuration.executor_timeout_ms {
            vars.insert(
                "NEWTON_EXECUTOR_TIMEOUT_MS".to_string(),
                exec_timeout.to_string(),
            );
        }
        vars.insert(
            "NEWTON_WORKSPACE_PATH".to_string(),
            workspace_path.display().to_string(),
        );
        vars.insert(
            "NEWTON_ITERATION_ID".to_string(),
            Uuid::new_v4().to_string(),
        );
        vars.insert(
            "NEWTON_EXECUTION_ID".to_string(),
            Uuid::new_v4().to_string(),
        );
        Self { vars }
    }

    fn env_refs(&self) -> Vec<(&str, &str)> {
        self.vars
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    fn metadata(&self) -> Vec<(String, String)> {
        self.vars
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn add_tool_vars(&mut self, tool: ToolType, cmd: &str, args: &[String]) {
        let arg_summary = args.join(" ");
        self.vars
            .insert("NEWTON_EXECUTING_TOOL".to_string(), format!("{:?}", tool));
        self.vars.insert(
            "NEWTON_EXECUTING_COMMAND".to_string(),
            format!("{} {}", cmd, arg_summary).trim().to_string(),
        );
    }
}

impl ToolExecutor {
    pub fn new() -> Self {
        ToolExecutor
    }

    pub async fn execute(
        &self,
        cmd: &str,
        configuration: &ExecutionConfiguration,
        workspace_path: &std::path::PathBuf,
    ) -> Result<ToolResult, AppError> {
        let (program, args) = parse_command(cmd)?;
        println!("Executing tool: {}", cmd);

        let mut env_builder =
            ToolExecutionEnv::from_configuration(configuration, workspace_path.as_path());
        env_builder.add_tool_vars(ToolType::Executor, cmd, &args);
        let env_refs = env_builder.env_refs();
        let env_metadata = env_builder.metadata();

        let start_time = Instant::now();
        let output = tokio::process::Command::new(program)
            .args(&args)
            .current_dir(workspace_path)
            .envs(env_refs)
            .output()
            .await
            .map_err(|e| {
                AppError::new(
                    crate::core::types::ErrorCategory::ToolExecutionError,
                    format!("Failed to execute tool: {}", e),
                )
                .with_code("TOOL-001")
            })?;

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(build_tool_result(
            cmd,
            output,
            execution_time_ms,
            args,
            env_metadata,
        ))
    }
}

impl Default for ToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(clippy::result_large_err)]
fn parse_command(cmd: &str) -> Result<(String, Vec<String>), AppError> {
    let mut parts = cmd.split_whitespace();
    let program = parts.next().ok_or_else(|| {
        AppError::new(
            crate::core::types::ErrorCategory::ToolExecutionError,
            "command must not be empty",
        )
        .with_code("TOOL-002")
    })?;
    let args = parts.map(|s| s.to_string()).collect();
    Ok((program.to_string(), args))
}

fn build_tool_result(
    cmd: &str,
    output: std::process::Output,
    execution_time_ms: u64,
    arguments: Vec<String>,
    environment_variables: Vec<(String, String)>,
) -> ToolResult {
    ToolResult {
        tool_name: cmd.to_string(),
        exit_code: output.status.code().unwrap_or(-1) as i32,
        execution_time_ms,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        success: output.status.success(),
        error: if output.status.success() {
            None
        } else {
            Some("Tool execution failed".to_string())
        },
        metadata: ToolMetadata {
            tool_version: None,
            tool_type: ToolType::Executor,
            arguments,
            environment_variables,
        },
    }
}
