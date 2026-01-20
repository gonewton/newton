use crate::{tools::ToolResult, Result, core::error::{AppError, ErrorCategory}};
use std::collections::HashMap;

pub async fn execute_command(
    cmd: &str,
    env: &HashMap<String, String>,
    timeout_ms: Option<u64>,
) -> Result<ToolResult> {
    let max_retries = 3;
    for attempt in 0..max_retries {
        let timeout_duration = timeout_ms.map(std::time::Duration::from_millis);
        let result = if let Some(duration) = timeout_duration {
            tokio::time::timeout(duration, tokio::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .envs(env)
                .output())
                .await
                .map_err(|_| AppError::new(ErrorCategory::TimeoutError, "Command timed out".to_string()))??
        } else {
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .envs(env)
                .output()
                .await
                .map_err(AppError::from)?
        };

        let success = result.status.success();
        let exit_code = result.status.code().unwrap_or(-1);
        let execution_time_ms = 100; // TODO: measure actual time
        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let stderr = String::from_utf8_lossy(&result.stderr).to_string();

        if success {
            return Ok(ToolResult {
                success,
                exit_code,
                execution_time_ms,
                stdout,
                stderr,
            });
        }

        // Retry with backoff if not success
        if attempt < max_retries - 1 {
            tokio::time::sleep(tokio::time::Duration::from_millis(500 * (attempt + 1) as u64)).await;
        }
    }

    // Return failure after retries
    Ok(ToolResult {
        success: false,
        exit_code: -1,
        execution_time_ms: 100,
        stdout: "".to_string(),
        stderr: "Command failed after retries".to_string(),
    })
}
