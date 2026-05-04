use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::ExecutionContext;
use crate::workflow::state::GraphSettings;
use std::path::{Path, PathBuf};

/// Resolved artifact paths for a single task run.
pub(super) struct ArtifactPaths {
    pub(super) task_artifact_dir: PathBuf,
    pub(super) stdout_abs: PathBuf,
    pub(super) stderr_abs: PathBuf,
    pub(super) stdout_rel: String,
    pub(super) stderr_rel: String,
}

/// Create the artifact directory for a task run and return the resolved paths.
pub(super) fn setup_artifact_paths(
    workspace_root: &Path,
    settings: &GraphSettings,
    ctx: &ExecutionContext,
) -> Result<ArtifactPaths, AppError> {
    let artifact_base = if settings.artifact_storage.base_path.is_absolute() {
        settings.artifact_storage.base_path.clone()
    } else {
        workspace_root.join(&settings.artifact_storage.base_path)
    };
    let run_seq = ctx.iteration as usize;
    let task_artifact_dir = artifact_base
        .join("workflows")
        .join(&ctx.execution_id)
        .join("task")
        .join(&ctx.task_id)
        .join(run_seq.to_string());
    let stdout_abs = task_artifact_dir.join("stdout.txt");
    let stderr_abs = task_artifact_dir.join("stderr.txt");
    std::fs::create_dir_all(&task_artifact_dir).map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to create artifact directory: {err}"),
        )
    })?;
    let stdout_rel = stdout_abs.strip_prefix(workspace_root).map_or_else(
        |_| stdout_abs.to_string_lossy().to_string(),
        |p| p.to_string_lossy().to_string(),
    );
    let stderr_rel = stderr_abs.strip_prefix(workspace_root).map_or_else(
        |_| stderr_abs.to_string_lossy().to_string(),
        |p| p.to_string_lossy().to_string(),
    );
    Ok(ArtifactPaths {
        task_artifact_dir,
        stdout_abs,
        stderr_abs,
        stdout_rel,
        stderr_rel,
    })
}

/// Open the stdout artifact file for writing.
pub(super) fn open_stdout_artifact_file(stdout_path: &Path) -> Result<std::fs::File, AppError> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(stdout_path)
        .map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to open stdout artifact: {err}"),
            )
        })
}
