use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::tools::ToolResult;

pub use crate::core::types::{
    ErrorCategory, ErrorSeverity, ExecutionStatus, IterationPhase, ToolType,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationExecution {
    pub id: Uuid,
    pub workspace_path: PathBuf,
    pub execution_id: Uuid,
    pub status: ExecutionStatus,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub resource_limits: ResourceLimits,
    pub max_iterations: Option<usize>,
    pub current_iteration: Option<usize>,
    pub final_solution_path: Option<PathBuf>,
    pub current_iteration_path: Option<PathBuf>,
    pub total_iterations_completed: usize,
    pub total_iterations_failed: usize,
    pub iterations: Vec<Iteration>,
    pub artifacts: Vec<ArtifactMetadata>,
    pub configuration: ExecutionConfiguration,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceLimits {
    pub max_iterations: Option<usize>,
    pub max_time_seconds: Option<u64>,
    pub max_memory_mb: Option<usize>,
    pub max_disk_space_mb: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Iteration {
    pub iteration_id: Uuid,
    pub execution_id: Uuid,
    pub iteration_number: usize,
    pub phase: IterationPhase,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub evaluator_result: Option<ToolResult>,
    pub advisor_result: Option<ToolResult>,
    pub executor_result: Option<ToolResult>,
    pub predecessor_solution: Option<PathBuf>,
    pub successor_solution: Option<PathBuf>,
    pub artifacts: Vec<ArtifactMetadata>,
    pub metadata: IterationMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    pub id: Uuid,
    pub execution_id: Option<Uuid>,
    pub iteration_id: Option<Uuid>,
    pub name: String,
    pub path: PathBuf,
    pub content_type: String,
    pub size_bytes: u64,
    pub created_at: i64,
    pub modified_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionConfiguration {
    pub evaluator_cmd: Option<String>,
    pub advisor_cmd: Option<String>,
    pub executor_cmd: Option<String>,
    pub evaluator_timeout_ms: Option<u64>,
    pub advisor_timeout_ms: Option<u64>,
    pub executor_timeout_ms: Option<u64>,
    pub global_timeout_ms: Option<u64>,
    pub max_iterations: Option<usize>,
    pub max_time_seconds: Option<u64>,
    pub strict_toolchain_mode: bool,
    pub resource_monitoring: bool,
    /// Enable verbose output for tool execution
    pub verbose: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetadata {
    pub tool_version: Option<String>,
    pub tool_type: ToolType,
    pub arguments: Vec<String>,
    pub environment_variables: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IterationMetadata {
    pub phase: IterationPhase,
    pub solution_file_path: Option<PathBuf>,
    pub report_path: Option<PathBuf>,
    pub artifacts_generated: usize,
    pub artifacts_deleted: usize,
}
