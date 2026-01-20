use crate::core::entities::ExecutionConfiguration;
use crate::core::error::{AppError, ErrorReporter};
use crate::core::logger::*;
use std::collections::HashMap;

pub struct ToolExecutor;

impl ToolExecutor {
    pub fn new() -> Self {
        ToolExecutor
    }

    pub async fn execute(
        &self,
        cmd: &str,
        configuration: &ExecutionConfiguration,
        workspace_path: &std::path::PathBuf,
    ) -> Result<crate::core::entities::ToolResult, AppError> {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let program = parts[0];
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

        self.report_debug(&format!("Executing tool: {}", cmd));

        let mut env_vars = HashMap::new();
        
        if let Some(eval_cmd) = &configuration.evaluator_cmd {
            env_vars.insert("NEWTON_EVALUATOR_CMD".to_string(), eval_cmd.clone());
        }
        if let Some(adv_cmd) = &configuration.advisor_cmd {
            env_vars.insert("NEWTON_ADVISOR_CMD".to_string(), adv_cmd.clone());
        }
        if let Some(exec_cmd) = &configuration.executor_cmd {
            env_vars.insert("NEWTON_EXECUTOR_CMD".to_string(), exec_cmd.clone());
        }
        if let Some(eval_timeout) = configuration.evaluator_timeout_ms {
            env_vars.insert("NEWTON_EVALUATOR_TIMEOUT_MS".to_string(), eval_timeout.to_string());
        }
        if let Some(adv_timeout) = configuration.advisor_timeout_ms {
            env_vars.insert("NEWTON_ADVISOR_TIMEOUT_MS".to_string(), adv_timeout.to_string());
        }
        if let Some(exec_timeout) = configuration.executor_timeout_ms {
            env_vars.insert("NEWTON_EXECUTOR_TIMEOUT_MS".to_string(), exec_timeout.to_string());
        }
        env_vars.insert("NEWTON_WORKSPACE_PATH".to_string(), workspace_path.display().to_string());
        env_vars.insert("NEWTON_ITERATION_ID".to_string(), uuid::Uuid::new_v4().to_string());
        env_vars.insert("NEWTON_EXECUTION_ID".to_string(), uuid::Uuid::new_v4().to_string());

        let env_vars: Vec<(&str, &str)> = env_vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

        let start_time = std::time::Instant::now();
        let output = tokio::process::Command::new(program)
            .args(&args)
            .current_dir(workspace_path)
            .envs(env_vars)
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

        Ok(crate::core::entities::ToolResult {
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
            metadata: crate::core::entities::ToolMetadata {
                tool_version: None,
                tool_type: crate::core::entities::ToolType::Executor,
                arguments: args,
                environment_variables: env_vars,
            },
        })
    }
}
