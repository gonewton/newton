#![allow(clippy::result_large_err)] // Checkpoint module returns AppError to preserve structured diagnostic context; boxing would discard run-time state.

use crate::core::error::AppError;
use crate::core::workflow_graph::state::{
    OutputRef, WorkflowCheckpoint, WorkflowExecution, WorkflowExecutionStatus,
};
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

/// Paths under `.newton/state/workflows/<execution_id>`.
pub struct WorkflowStatePaths {
    pub execution_dir: PathBuf,
    pub execution_file: PathBuf,
    pub checkpoint_file: PathBuf,
    pub checkpoints_dir: PathBuf,
}

impl WorkflowStatePaths {
    pub fn new(workspace_root: &Path, execution_id: &Uuid) -> Self {
        let base = workspace_root.join(".newton/state/workflows");
        let execution_dir = base.join(execution_id.to_string());
        let execution_file = execution_dir.join("execution.json");
        let checkpoint_file = execution_dir.join("checkpoint.json");
        let checkpoints_dir = execution_dir.join("checkpoints");
        Self {
            execution_dir,
            execution_file,
            checkpoint_file,
            checkpoints_dir,
        }
    }

    pub fn workspace_root(workspace_root: &Path) -> PathBuf {
        workspace_root.join(".newton/state/workflows")
    }
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!("failed to create directory {}: {}", parent.display(), err),
            )
        })?;
    }
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, data).map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::IoError,
            format!("failed to write {}: {}", tmp_path.display(), err),
        )
    })?;
    fs::rename(&tmp_path, path).map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::IoError,
            format!(
                "failed to rename {} -> {}: {}",
                tmp_path.display(),
                path.display(),
                err
            ),
        )
    })?;
    Ok(())
}

pub fn save_execution(
    workspace_root: &Path,
    execution_id: &Uuid,
    execution: &WorkflowExecution,
) -> Result<(), AppError> {
    let paths = WorkflowStatePaths::new(workspace_root, execution_id);
    let content = serde_json::to_vec_pretty(execution).map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::SerializationError,
            format!("failed to serialize execution.json: {}", err),
        )
    })?;
    atomic_write(&paths.execution_file, &content)
}

pub fn save_checkpoint(
    workspace_root: &Path,
    execution_id: &Uuid,
    checkpoint: &WorkflowCheckpoint,
    keep_history: bool,
) -> Result<(), AppError> {
    let paths = WorkflowStatePaths::new(workspace_root, execution_id);
    let content = serde_json::to_vec_pretty(checkpoint).map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::SerializationError,
            format!("failed to serialize checkpoint.json: {}", err),
        )
    })?;
    atomic_write(&paths.checkpoint_file, &content)?;
    if keep_history {
        if !paths.checkpoints_dir.exists() {
            fs::create_dir_all(&paths.checkpoints_dir).map_err(|err| {
                AppError::new(
                    crate::core::types::ErrorCategory::IoError,
                    format!(
                        "failed to create checkpoints dir {}: {}",
                        paths.checkpoints_dir.display(),
                        err
                    ),
                )
            })?;
        }
        let timestamp = checkpoint
            .created_at
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            .replace(':', "-");
        let historic = paths
            .checkpoints_dir
            .join(format!("checkpoint-{}.json", timestamp));
        atomic_write(&historic, &content)?;
    }
    Ok(())
}

pub fn load_execution(
    workspace_root: &Path,
    execution_id: &Uuid,
) -> Result<WorkflowExecution, AppError> {
    let paths = WorkflowStatePaths::new(workspace_root, execution_id);
    let bytes = fs::read(&paths.execution_file).map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::IoError,
            format!("failed to read {}: {}", paths.execution_file.display(), err),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::SerializationError,
            format!("failed to deserialize execution.json: {}", err),
        )
    })
}

pub fn load_checkpoint(
    workspace_root: &Path,
    execution_id: &Uuid,
) -> Result<WorkflowCheckpoint, AppError> {
    let paths = WorkflowStatePaths::new(workspace_root, execution_id);
    let bytes = fs::read(&paths.checkpoint_file).map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::IoError,
            format!(
                "failed to read {}: {}",
                paths.checkpoint_file.display(),
                err
            ),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|err| {
        AppError::new(
            crate::core::types::ErrorCategory::SerializationError,
            format!("failed to deserialize checkpoint.json: {}", err),
        )
    })
}

pub struct CheckpointSummary {
    pub execution_id: Uuid,
    pub status: WorkflowExecutionStatus,
    pub started_at: DateTime<Utc>,
    pub checkpoint_age: Duration,
    pub checkpoint_size: u64,
}

pub fn list_checkpoints(workspace_root: &Path) -> Result<Vec<CheckpointSummary>, AppError> {
    let mut entries = Vec::new();
    let base = WorkflowStatePaths::workspace_root(workspace_root);
    if !base.exists() {
        return Ok(entries);
    }
    for entry in fs::read_dir(&base)
        .map_err(|err| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!("failed to list workflows state: {}", err),
            )
        })?
        .flatten()
    {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        if let Ok(uuid) = Uuid::parse_str(&entry.file_name().to_string_lossy()) {
            if let (Ok(execution), Ok(metadata)) = (
                load_execution(workspace_root, &uuid),
                fs::metadata(
                    WorkflowStatePaths::new(workspace_root, &uuid)
                        .checkpoint_file
                        .clone(),
                ),
            ) {
                if let Ok(modified) = metadata.modified() {
                    let now = SystemTime::now();
                    let age = now
                        .duration_since(modified)
                        .unwrap_or_else(|_| Duration::from_secs(0));
                    entries.push(CheckpointSummary {
                        execution_id: uuid,
                        status: execution.status,
                        started_at: execution.started_at,
                        checkpoint_age: age,
                        checkpoint_size: metadata.len(),
                    });
                }
            }
        }
    }
    Ok(entries)
}

pub fn clean_checkpoints(workspace_root: &Path, older_than: Duration) -> Result<(), AppError> {
    let base = WorkflowStatePaths::workspace_root(workspace_root);
    if !base.exists() {
        return Ok(());
    }
    let now = SystemTime::now();
    for entry in fs::read_dir(&base)
        .map_err(|err| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!("failed to list workflows state: {}", err),
            )
        })?
        .flatten()
    {
        let checkpoints_dir = entry.path().join("checkpoints");
        if !checkpoints_dir.is_dir() {
            continue;
        }
        for item in fs::read_dir(&checkpoints_dir)
            .map_err(|err| {
                AppError::new(
                    crate::core::types::ErrorCategory::IoError,
                    format!("failed to scan checkpoints dir: {}", err),
                )
            })?
            .flatten()
        {
            if let Ok(metadata) = item.metadata() {
                if let Ok(modified) = metadata.modified() {
                    if now
                        .duration_since(modified)
                        .unwrap_or_else(|_| Duration::from_secs(0))
                        >= older_than
                    {
                        let _ = fs::remove_file(item.path());
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn collect_live_artifact_paths(
    workspace_root: &Path,
    retention: Duration,
) -> Result<HashSet<PathBuf>, AppError> {
    let mut live = HashSet::new();
    let base = WorkflowStatePaths::workspace_root(workspace_root);
    if !base.exists() {
        return Ok(live);
    }
    let now = SystemTime::now();
    for entry in fs::read_dir(&base)
        .map_err(|err| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!("failed to list workflows state: {}", err),
            )
        })?
        .flatten()
    {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if let Ok(exec_id) = Uuid::parse_str(&file_name) {
            let paths = WorkflowStatePaths::new(workspace_root, &exec_id);
            let checkpoint_meta = match fs::metadata(&paths.checkpoint_file) {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            let checkpoint_age = now
                .duration_since(checkpoint_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH))
                .unwrap_or_else(|_| Duration::from_secs(0));
            let execution_status = load_execution(workspace_root, &exec_id).map(|exec| exec.status);
            let status_protect = matches!(
                execution_status,
                Ok(WorkflowExecutionStatus::Running | WorkflowExecutionStatus::Cancelled)
            ) || checkpoint_age <= retention;
            if !status_protect {
                continue;
            }
            if let Ok(checkpoint) = load_checkpoint(workspace_root, &exec_id) {
                for record in checkpoint.completed.values() {
                    if let OutputRef::Artifact { path, .. } = &record.output_ref {
                        let absolute = workspace_root.join(path);
                        if let Ok(canonical) = absolute.canonicalize() {
                            live.insert(canonical);
                        }
                    }
                }
            }
        }
    }
    Ok(live)
}
