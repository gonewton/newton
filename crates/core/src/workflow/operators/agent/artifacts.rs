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

/// Best-effort append of a `[capture truncated: <reason>]` marker line to an
/// on-disk capture artifact after some of its writes were skipped (hit
/// `OUTPUT_CAPTURE_LIMIT_BYTES`) or failed (I/O error) — see spec 074 S15.
///
/// This is itself diagnostic, not load-bearing: if the append also fails
/// (e.g. the same disk-full condition that caused the original truncation),
/// log and move on rather than propagating another error out of an
/// already-degraded capture path.
pub(super) fn append_capture_truncation_marker(path: &Path, reason: &str) {
    use std::io::Write as _;
    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(f) => f,
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "AgentOperator: failed to open capture artifact to append truncation marker"
            );
            return;
        }
    };
    if let Err(err) = writeln!(file, "[capture truncated: {reason}]") {
        tracing::warn!(
            path = %path.display(),
            error = %err,
            "AgentOperator: failed to append capture-truncation marker to artifact"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn append_capture_truncation_marker_writes_marker_line_on_success() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stdout.txt");
        std::fs::write(&path, "existing output\n").unwrap();

        append_capture_truncation_marker(&path, "output exceeded 1048576 byte capture limit");

        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            contents,
            "existing output\n[capture truncated: output exceeded 1048576 byte capture limit]\n"
        );
    }

    /// Covers the best-effort open-failure branch: this is diagnostic-only
    /// (spec 074 S15 doc comment above), so a missing parent directory must
    /// be swallowed — logged, not panicked, and no file/dir created as a
    /// side effect.
    #[test]
    fn append_capture_truncation_marker_does_not_panic_when_open_fails() {
        let tmp = TempDir::new().unwrap();
        let unopenable_path = tmp.path().join("missing_parent_dir").join("stdout.txt");

        append_capture_truncation_marker(&unopenable_path, "some reason");

        assert!(
            !unopenable_path.exists(),
            "no file should be created when the parent directory is missing"
        );
    }
}
