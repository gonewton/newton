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

/// Write `text` (plus a trailing newline) to `file`, honoring
/// `OUTPUT_CAPTURE_LIMIT_BYTES` and surfacing any I/O failure as a
/// truncation reason instead of dropping it silently.
///
/// Shared by the SDK engine path (`sdk.rs::execute_sdk_engine`, three call
/// sites: stdout, stderr `RawLine`, stderr `JsonLine`) and the subprocess
/// engine path (`command.rs::stream_and_process_output`, two call sites:
/// the `StreamJson` non-text branch and the main text branch) — both
/// previously duplicated this same byte-cap/write-error logic inline. See
/// spec 074 S15.
///
/// Extracted as a standalone, deterministic function — given a
/// `bytes_so_far` and `text`, no subprocess or SDK event stream involved —
/// so its byte-cap and write-error branches are directly unit-testable
/// without needing a real subprocess or a real installed agent binary.
///
/// Returns the updated byte count (unchanged when the cap was already hit)
/// and the truncation reason to carry forward (first cause wins across
/// repeated calls within the same iteration — pass the previous call's
/// return value back in as `existing_warning`).
pub(super) fn write_capture_chunk(
    file: &mut std::fs::File,
    path: &Path,
    bytes_so_far: usize,
    text: &str,
    existing_warning: Option<String>,
    stream_label: &str,
) -> (usize, Option<String>) {
    use crate::workflow::operators::OUTPUT_CAPTURE_LIMIT_BYTES;
    use std::io::Write;

    if bytes_so_far + text.len() < OUTPUT_CAPTURE_LIMIT_BYTES {
        let mut warning = existing_warning;
        if let Err(err) = file
            .write_all(text.as_bytes())
            .and_then(|()| file.write_all(b"\n"))
        {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "AgentOperator: failed to write {stream_label} capture artifact"
            );
            if warning.is_none() {
                warning = Some(format!("write error: {err}"));
            }
        }
        (bytes_so_far + text.len() + 1, warning)
    } else {
        let warning = existing_warning.or_else(|| {
            Some(format!(
                "output exceeded {OUTPUT_CAPTURE_LIMIT_BYTES} byte capture limit"
            ))
        });
        (bytes_so_far, warning)
    }
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
    use crate::workflow::operators::OUTPUT_CAPTURE_LIMIT_BYTES;
    use tempfile::TempDir;

    // `write_capture_chunk` tests below (moved from `sdk.rs` when the
    // duplicated stdout/stderr write-and-truncation-check blocks in
    // `command.rs::stream_and_process_output` were deduplicated onto this
    // shared function — spec 074 S15, tranche 4 code review). Both the SDK
    // engine path (`sdk.rs::execute_sdk_engine`, three call sites) and the
    // subprocess engine path (`command.rs::stream_and_process_output`, two
    // call sites) drive real event streams / subprocess output that would
    // require a real installed agent binary or a real long-running
    // subprocess to exercise the byte-cap and write-error branches
    // end-to-end. `write_capture_chunk` was extracted as the standalone,
    // deterministic core of those branches — no event stream or subprocess
    // involved — so it can be exercised directly here with a real temp
    // file.

    fn open_append(path: &Path) -> std::fs::File {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap()
    }

    #[test]
    fn write_capture_chunk_writes_text_and_advances_byte_count_on_success() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stdout.log");
        let mut file = open_append(&path);

        let (new_bytes, warning) =
            write_capture_chunk(&mut file, &path, 0, "hello world", None, "stdout");

        assert_eq!(new_bytes, "hello world".len() + 1);
        assert!(warning.is_none());
        drop(file);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "hello world\n");
    }

    #[test]
    fn write_capture_chunk_returns_cap_exceeded_warning_without_writing() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stdout.log");
        let mut file = open_append(&path);

        // Seed `bytes_so_far` right at the cap so even a short chunk trips
        // it — no need to actually write a megabyte of data to exercise
        // this arithmetic.
        let (new_bytes, warning) = write_capture_chunk(
            &mut file,
            &path,
            OUTPUT_CAPTURE_LIMIT_BYTES,
            "one more line",
            None,
            "stdout",
        );

        assert_eq!(
            new_bytes, OUTPUT_CAPTURE_LIMIT_BYTES,
            "byte count must not advance once the cap is hit"
        );
        let warning = warning.expect("cap-exceeded must produce a warning");
        assert!(
            warning.contains("byte capture limit"),
            "unexpected warning: {warning}"
        );
        drop(file);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(
            contents.is_empty(),
            "text must not be written once the cap is exceeded"
        );
    }

    #[test]
    fn write_capture_chunk_preserves_first_warning_across_repeated_calls() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stderr.log");
        let mut file = open_append(&path);

        let (bytes1, warning1) = write_capture_chunk(
            &mut file,
            &path,
            OUTPUT_CAPTURE_LIMIT_BYTES,
            "first",
            None,
            "stderr",
        );
        let (_bytes2, warning2) = write_capture_chunk(
            &mut file,
            &path,
            bytes1,
            "second",
            warning1.clone(),
            "stderr",
        );

        assert_eq!(
            warning1, warning2,
            "the first truncation cause must win across repeated calls"
        );
    }

    #[test]
    fn write_capture_chunk_returns_write_error_reason_when_write_fails() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("stdout.log");
        std::fs::write(&path, b"").unwrap();
        // Open read-only: `write_all` on this handle fails with EBADF, a
        // reliable and portable way to exercise the write-error branch
        // without any fragile OS-level permission trick.
        let mut file = std::fs::File::open(&path).unwrap();

        let (new_bytes, warning) =
            write_capture_chunk(&mut file, &path, 0, "unwritable", None, "stdout");

        // The byte count still advances even though the write failed —
        // matches the pre-extraction behavior (the counter tracks
        // "attempted", not "succeeded", writes so a string of failures
        // doesn't wedge the cap check into never firing).
        assert_eq!(new_bytes, "unwritable".len() + 1);
        let warning = warning.expect("write failure must produce a warning");
        assert!(
            warning.starts_with("write error:"),
            "unexpected warning: {warning}"
        );
    }

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
