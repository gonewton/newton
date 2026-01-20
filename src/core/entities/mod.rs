use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationExecution {
    pub id: Uuid,
    pub workspace_id: String,
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
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub path: PathBuf,
    pub configuration: WorkspaceConfiguration,
    pub template_id: Option<String>,
    pub status: WorkspaceStatus,
    pub created_at: i64,
    pub updated_at: Option<i64>,
    pub last_used: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorRecord {
    pub id: Uuid,
    pub execution_id: Option<Uuid>,
    pub iteration_id: Option<Uuid>,
    pub workspace_id: Option<String>,
    pub error_category: ErrorCategory,
    pub severity: ErrorSeverity,
    pub error_code: String,
    pub message: String,
    pub context: ErrorContext,
    pub recovery_suggestions: Vec<String>,
    pub occurred_at: DateTime<Utc>,
    pub stack_trace: Option<String>,
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
    pub metadata: ToolMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    pub id: Uuid,
    pub execution_id: Option<Uuid>,
    pub iteration_id: Option<Uuid>,
    pub workspace_id: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceLimits {
    pub max_iterations: Option<usize>,
    pub max_time_seconds: Option<u64>,
    pub max_memory_mb: Option<usize>,
    pub max_disk_space_mb: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfiguration {
    pub name: String,
    pub description: Option<String>,
    pub template_id: Option<String>,
    pub parameters: Vec<Parameter>,
    pub settings: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub value: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IterationMetadata {
    pub phase: IterationPhase,
    pub solution_file_path: Option<PathBuf>,
    pub report_path: Option<PathBuf>,
    pub artifacts_generated: usize,
    pub artifacts_deleted: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorContext {
    pub location: ErrorLocation,
    pub component: String,
    pub details: HashMap<String, String>,
    pub related_artifacts: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorLocation {
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub function: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetadata {
    pub tool_version: Option<String>,
    pub tool_type: ToolType,
    pub arguments: Vec<String>,
    pub environment_variables: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkspaceStatus {
    Valid,
    Invalid,
    Locked,
    Processing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IterationPhase {
    Evaluator,
    Advisor,
    Executor,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorCategory {
    ValidationError,
    ToolExecutionError,
    TimeoutError,
    ResourceError,
    WorkspaceError,
    IterationError,
    SerializationError,
    IoError,
    InternalError,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorSeverity {
    Error,
    Warning,
    Info,
    Debug,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolType {
    Evaluator,
    Advisor,
    Executor,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimization_execution_creation() {
        let execution = OptimizationExecution {
            id: Uuid::new_v4(),
            workspace_id: "test".to_string(),
            workspace_path: PathBuf::from("/tmp/test"),
            execution_id: Uuid::new_v4(),
            status: ExecutionStatus::Running,
            started_at: Utc::now(),
            completed_at: None,
            resource_limits: Default::default(),
            max_iterations: None,
            current_iteration: None,
            final_solution_path: None,
            current_iteration_path: None,
            total_iterations_completed: 0,
            total_iterations_failed: 0,
            iterations: Vec::new(),
            artifacts: Vec::new(),
            configuration: Default::default(),
        };
        assert_eq!(execution.id, execution.id);
    }
}
