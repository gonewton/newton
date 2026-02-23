#![allow(clippy::result_large_err)] // State module returns AppError to preserve structured diagnostic context; boxing would discard run-time state.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::schema::WorkflowSettings;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Version embedded in persisted workflow execution files.
pub const WORKFLOW_EXECUTION_FORMAT_VERSION: &str = "1";
/// Version embedded in persisted workflow checkpoint files.
pub const WORKFLOW_CHECKPOINT_FORMAT_VERSION: &str = "1";

pub type GraphSettings = WorkflowSettings;
fn default_trigger_payload_value() -> Value {
    Value::Object(Map::new())
}
/// Workflow execution metadata persisted under `.newton/state/workflows`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowExecution {
    pub format_version: String,
    pub execution_id: Uuid,
    pub workflow_file: String,
    pub workflow_version: String,
    pub workflow_hash: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: WorkflowExecutionStatus,
    pub settings_effective: GraphSettings,
    #[serde(default = "default_trigger_payload_value")]
    pub trigger_payload: Value,
    #[serde(default)]
    pub task_runs: Vec<WorkflowTaskRunSummary>,
}

/// Execution status enumeration for workflow graphs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowExecutionStatus {
    #[default]
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl WorkflowExecutionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowExecutionStatus::Running => "Running",
            WorkflowExecutionStatus::Completed => "Completed",
            WorkflowExecutionStatus::Failed => "Failed",
            WorkflowExecutionStatus::Cancelled => "Cancelled",
        }
    }
}

/// Workflow task status for persisted records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowTaskStatus {
    Success,
    Failed,
    Skipped,
}

/// Lightweight per-task summary appended to `execution.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTaskRunSummary {
    pub task_id: String,
    pub run_seq: usize,
    pub status: WorkflowTaskStatus,
    pub duration_ms: u64,
    pub error_code: Option<String>,
}

/// Detailed run record stored in checkpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTaskRunRecord {
    pub task_id: String,
    pub run_seq: usize,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub status: WorkflowTaskStatus,
    pub output_ref: OutputRef,
    pub error: Option<AppErrorSummary>,
}

/// Simplified summary of errors persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppErrorSummary {
    pub code: String,
    pub category: String,
    pub message: String,
}

/// Representation of operator output in checkpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum OutputRef {
    Inline(Value),
    Artifact {
        path: PathBuf,
        size_bytes: u64,
        sha256: String,
    },
}

impl OutputRef {
    pub fn materialize(&self, workspace_root: &Path) -> Result<Value, AppError> {
        match self {
            OutputRef::Inline(value) => Ok(value.clone()),
            OutputRef::Artifact { path, .. } => {
                let absolute = workspace_root.join(path);
                let bytes = fs::read(&absolute).map_err(|err| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!("failed to read artifact {}: {}", absolute.display(), err),
                    )
                })?;
                serde_json::from_slice(&bytes).map_err(|err| {
                    AppError::new(
                        ErrorCategory::SerializationError,
                        format!(
                            "failed to deserialize artifact {}: {}",
                            absolute.display(),
                            err
                        ),
                    )
                })
            }
        }
    }
}

/// Workflow checkpoint persisted to `checkpoint.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCheckpoint {
    pub format_version: String,
    pub execution_id: Uuid,
    pub workflow_hash: String,
    pub created_at: DateTime<Utc>,
    pub ready_queue: Vec<String>,
    pub context: Value,
    #[serde(default = "default_trigger_payload_value")]
    pub trigger_payload: Value,
    pub task_iterations: HashMap<String, usize>,
    pub total_iterations: usize,
    pub completed: HashMap<String, WorkflowTaskRunRecord>,
}

impl WorkflowCheckpoint {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        execution_id: Uuid,
        workflow_hash: String,
        context: Value,
        trigger_payload: Value,
        ready_queue: Vec<String>,
        task_iterations: HashMap<String, usize>,
        total_iterations: usize,
        completed: HashMap<String, WorkflowTaskRunRecord>,
    ) -> Self {
        WorkflowCheckpoint {
            format_version: WORKFLOW_CHECKPOINT_FORMAT_VERSION.to_string(),
            execution_id,
            workflow_hash,
            created_at: Utc::now(),
            ready_queue,
            context,
            trigger_payload,
            task_iterations,
            total_iterations,
            completed,
        }
    }
}

impl WorkflowTaskStatus {
    pub fn from_execution(status: crate::core::workflow_graph::executor::TaskStatus) -> Self {
        match status {
            crate::core::workflow_graph::executor::TaskStatus::Success => {
                WorkflowTaskStatus::Success
            }
            crate::core::workflow_graph::executor::TaskStatus::Failed => WorkflowTaskStatus::Failed,
            crate::core::workflow_graph::executor::TaskStatus::Skipped => {
                WorkflowTaskStatus::Skipped
            }
        }
    }
}

impl From<WorkflowTaskRunRecord> for WorkflowTaskRunSummary {
    fn from(record: WorkflowTaskRunRecord) -> Self {
        WorkflowTaskRunSummary {
            task_id: record.task_id,
            run_seq: record.run_seq,
            status: record.status,
            duration_ms: record
                .completed_at
                .signed_duration_since(record.started_at)
                .num_milliseconds() as u64,
            error_code: record.error.as_ref().map(|err| err.code.clone()),
        }
    }
}

impl WorkflowTaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowTaskStatus::Success => "success",
            WorkflowTaskStatus::Failed => "failed",
            WorkflowTaskStatus::Skipped => "skipped",
        }
    }
}

/// Redact sensitive keys in the given JSON value.
pub fn redact_value(value: &mut Value, redact_keys: &[String]) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if should_redact(key, redact_keys) {
                    *child = Value::String("[REDACTED]".to_string());
                    continue;
                }
                redact_value(child, redact_keys);
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_value(item, redact_keys);
            }
        }
        _ => {}
    }
}

fn should_redact(key: &str, redact_keys: &[String]) -> bool {
    let key_lower = key.to_lowercase();
    for pattern in redact_keys {
        if key_lower.contains(&pattern.to_lowercase()) {
            return true;
        }
    }
    false
}

/// Create a redacted summary of an AppError.
pub fn summarize_error(error: &AppError, redact_keys: &[String]) -> AppErrorSummary {
    let mut message = error.message.clone();
    for pattern in redact_keys {
        if error
            .context
            .get("context")
            .map(|ctx| ctx.to_lowercase().contains(&pattern.to_lowercase()))
            .unwrap_or(false)
        {
            message = "[REDACTED]".to_string();
            break;
        }
    }
    AppErrorSummary {
        code: error.code.clone(),
        category: format!("{:?}", error.category),
        message,
    }
}

/// Compute the SHA-256 hash encoded as lowercase hex.
pub fn compute_sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Determine if a task_id is valid for filesystem paths.
pub fn validate_task_id(task_id: &str) -> Result<(), AppError> {
    if task_id.contains('/') || task_id.contains('\\') || task_id.contains("..") {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "task_id contains invalid characters for filesystem use",
        )
        .with_code("WFG-ART-001"));
    }
    if !task_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "task_id contains invalid characters for filesystem use",
        )
        .with_code("WFG-ART-001"));
    }
    Ok(())
}

/// Build the canonical absolute workflow file path.
pub fn canonicalize_workflow_path(path: &Path) -> Result<PathBuf, AppError> {
    path.canonicalize().map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!(
                "failed to canonicalize workflow path {}: {}",
                path.display(),
                err
            ),
        )
    })
}
