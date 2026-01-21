pub mod execution;
pub mod strict_toolchain;

use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[allow(async_fn_in_trait)]
pub trait Tool {
    async fn execute(&self, env: &HashMap<String, String>) -> Result<ToolResult>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_name: String,
    pub exit_code: i32,
    pub execution_time_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub error: Option<String>,
    pub metadata: crate::core::entities::ToolMetadata,
}
