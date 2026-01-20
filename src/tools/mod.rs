pub mod execution;
pub mod strict_toolchain;

use crate::Result;
use std::collections::HashMap;

pub trait Tool {
    async fn execute(&self, env: &HashMap<String, String>) -> Result<ToolResult>;
}

pub struct ToolResult {
    pub success: bool,
    pub exit_code: i32,
    pub execution_time_ms: u64,
    pub stdout: String,
    pub stderr: String,
}
