#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::state::compute_sha256_hex;
use chrono::{DateTime, Utc};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct WorkflowFileRecord {
    pub name: String,
    pub content: String,
    pub content_hash: String,
    pub size_bytes: u64,
    pub modified_at: DateTime<Utc>,
}

#[derive(Debug)]
pub enum WriteOutcome {
    Created,
    Updated,
}

pub trait WorkflowFileStore: Send + Sync {
    fn list(&self) -> Result<Vec<WorkflowFileRecord>, AppError>;
    fn read(&self, name: &str) -> Result<WorkflowFileRecord, AppError>;
    fn write(
        &self,
        name: &str,
        content: &str,
        if_match: Option<&str>,
    ) -> Result<WriteOutcome, AppError>;
    fn delete(&self, name: &str) -> Result<(), AppError>;
}

pub struct FsWorkflowFileStore {
    workflows_dir: PathBuf,
}

impl FsWorkflowFileStore {
    pub fn new(workflows_dir: PathBuf) -> Self {
        Self { workflows_dir }
    }
}

fn validate_slug(name: &str) -> Result<(), AppError> {
    // Reject trailing .yaml
    if name.ends_with(".yaml") || name.ends_with(".yml") {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!("workflow name '{name}' must not include .yaml extension"),
        )
        .with_code("ERR_VALIDATION"));
    }
    // Reject standalone . or ..
    if name == "." || name == ".." {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!("workflow name '{name}' is invalid"),
        )
        .with_code("ERR_VALIDATION"));
    }
    // Reject path separators
    if name.contains('/') || name.contains('\\') {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!("workflow name '{name}' must not contain path separators"),
        )
        .with_code("ERR_VALIDATION"));
    }
    // Reject .. segments anywhere
    if name.contains("..") {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!("workflow name '{name}' must not contain '..'"),
        )
        .with_code("ERR_VALIDATION"));
    }
    // Validate regex: ^[A-Za-z0-9._-]+$
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "workflow name '{name}' contains invalid characters; only [A-Za-z0-9._-] allowed"
            ),
        )
        .with_code("ERR_VALIDATION"));
    }
    Ok(())
}

fn resolve_and_check_path(workflows_dir: &Path, name: &str) -> Result<PathBuf, AppError> {
    validate_slug(name)?;
    let target = workflows_dir.join(format!("{name}.yaml"));

    // Canonicalize the workflows_dir (create if absent for write operations)
    // For read operations, if dir doesn't exist, the file won't exist either.
    // We do traversal check on the non-canonical path first by checking no ".." components.
    // Then if the dir exists, do a canonical check.
    if workflows_dir.exists() {
        let canonical_dir = fs::canonicalize(workflows_dir).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to canonicalize workflows dir: {e}"),
            )
        })?;
        if target.exists() {
            let canonical_target = fs::canonicalize(&target).map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to canonicalize path: {e}"),
                )
            })?;
            if !canonical_target.starts_with(&canonical_dir) {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "path traversal detected",
                )
                .with_code("ERR_VALIDATION"));
            }
        }
    }
    Ok(target)
}

fn system_time_to_datetime(t: SystemTime) -> DateTime<Utc> {
    let duration = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    DateTime::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos()).unwrap_or_default()
}

/// Durably persists `data` to `path` via the shared
/// [`crate::fs_util::atomic_write`] helper (write-temp, fsync, rename, fsync
/// parent dir), mapping any I/O failure into this module's [`AppError`]
/// shape so callers keep the diagnostics they had before the write was
/// consolidated into the shared helper.
fn atomic_write(path: &Path, data: &[u8]) -> Result<(), AppError> {
    crate::fs_util::atomic_write(path, data).map_err(|e| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to atomically write {}: {e}", path.display()),
        )
    })
}

impl WorkflowFileStore for FsWorkflowFileStore {
    fn list(&self) -> Result<Vec<WorkflowFileRecord>, AppError> {
        if !self.workflows_dir.exists() {
            return Ok(vec![]);
        }
        let mut records = Vec::new();
        let entries = fs::read_dir(&self.workflows_dir).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to read workflows dir: {e}"),
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                AppError::new(ErrorCategory::IoError, format!("directory read error: {e}"))
            })?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            let file_name = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let bytes = fs::read(&path).map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to read {}: {e}", path.display()),
                )
            })?;
            let content = String::from_utf8_lossy(&bytes).to_string();
            let content_hash = compute_sha256_hex(&bytes);
            let metadata = fs::metadata(&path).map_err(|e| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to stat {}: {e}", path.display()),
                )
            })?;
            let size_bytes = metadata.len();
            let modified_at = metadata
                .modified()
                .map(system_time_to_datetime)
                .unwrap_or_default();
            records.push(WorkflowFileRecord {
                name: file_name,
                content,
                content_hash,
                size_bytes,
                modified_at,
            });
        }
        Ok(records)
    }

    fn read(&self, name: &str) -> Result<WorkflowFileRecord, AppError> {
        let path = resolve_and_check_path(&self.workflows_dir, name)?;
        if !path.exists() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!("workflow file '{name}.yaml' not found"),
            )
            .with_code("ERR_NOT_FOUND"));
        }
        let bytes = fs::read(&path).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to read {}: {e}", path.display()),
            )
        })?;
        let content = String::from_utf8_lossy(&bytes).to_string();
        let content_hash = compute_sha256_hex(&bytes);
        let metadata = fs::metadata(&path).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to stat {}: {e}", path.display()),
            )
        })?;
        let size_bytes = metadata.len();
        let modified_at = metadata
            .modified()
            .map(system_time_to_datetime)
            .unwrap_or_default();
        Ok(WorkflowFileRecord {
            name: name.to_string(),
            content,
            content_hash,
            size_bytes,
            modified_at,
        })
    }

    fn write(
        &self,
        name: &str,
        content: &str,
        if_match: Option<&str>,
    ) -> Result<WriteOutcome, AppError> {
        validate_slug(name)?;
        // Ensure dir exists before path resolution
        fs::create_dir_all(&self.workflows_dir).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to create workflows dir: {e}"),
            )
        })?;
        let path = self.workflows_dir.join(format!("{name}.yaml"));
        // Traversal check after dir creation
        let canonical_dir = fs::canonicalize(&self.workflows_dir).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to canonicalize workflows dir: {e}"),
            )
        })?;
        // For traversal check we compare based on the expected path
        let target = canonical_dir.join(format!("{name}.yaml"));
        if !target.starts_with(&canonical_dir) {
            return Err(
                AppError::new(ErrorCategory::ValidationError, "path traversal detected")
                    .with_code("ERR_VALIDATION"),
            );
        }

        let existed = path.exists();
        // Optimistic concurrency check
        if let Some(expected) = if_match {
            if existed {
                let current_bytes = fs::read(&path).map_err(|e| {
                    AppError::new(
                        ErrorCategory::IoError,
                        format!("failed to read existing file: {e}"),
                    )
                })?;
                let current_hash = compute_sha256_hex(&current_bytes);
                if current_hash != expected {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        "ETag mismatch: file was modified by another writer",
                    )
                    .with_code("ERR_CONFLICT"));
                }
            }
        }
        atomic_write(&path, content.as_bytes())?;
        if existed {
            Ok(WriteOutcome::Updated)
        } else {
            Ok(WriteOutcome::Created)
        }
    }

    fn delete(&self, name: &str) -> Result<(), AppError> {
        let path = resolve_and_check_path(&self.workflows_dir, name)?;
        fs::remove_file(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppError::new(
                    ErrorCategory::ValidationError,
                    format!("workflow file '{name}.yaml' not found"),
                )
                .with_code("ERR_NOT_FOUND")
            } else {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to delete {}: {e}", path.display()),
                )
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_store(dir: &tempfile::TempDir) -> FsWorkflowFileStore {
        FsWorkflowFileStore::new(dir.path().to_owned())
    }

    // Slug validation tests
    #[test]
    fn test_slug_valid() {
        assert!(validate_slug("my-flow").is_ok());
        assert!(validate_slug("workflow_1").is_ok());
        assert!(validate_slug("flow.v2").is_ok());
    }

    #[test]
    fn test_slug_with_slash_rejected() {
        assert!(validate_slug("a/b").is_err());
    }

    #[test]
    fn test_slug_with_dotdot_rejected() {
        assert!(validate_slug("..").is_err());
        assert!(validate_slug("a..b").is_err());
    }

    #[test]
    fn test_slug_with_trailing_yaml_rejected() {
        assert!(validate_slug("demo.yaml").is_err());
    }

    #[test]
    fn test_slug_with_backslash_rejected() {
        assert!(validate_slug("a\\b").is_err());
    }

    #[test]
    fn test_slug_empty_rejected() {
        assert!(validate_slug("").is_err());
    }

    // Round-trip
    #[test]
    fn test_round_trip_write_read() {
        let dir = tempdir().unwrap();
        let store = make_store(&dir);
        let content = "version: \"1\"\nmode: graph\nworkflow:\n  tasks: []\n";
        store.write("my-flow", content, None).unwrap();
        let record = store.read("my-flow").unwrap();
        assert_eq!(record.content, content);
        assert_eq!(record.name, "my-flow");
    }

    #[test]
    fn test_hash_stability() {
        let dir = tempdir().unwrap();
        let store = make_store(&dir);
        let content = "version: \"1\"\n";
        store.write("flow1", content, None).unwrap();
        store.write("flow2", content, None).unwrap();
        let r1 = store.read("flow1").unwrap();
        let r2 = store.read("flow2").unwrap();
        assert_eq!(r1.content_hash, r2.content_hash);
    }

    #[test]
    fn test_traversal_rejection() {
        let dir = tempdir().unwrap();
        let store = make_store(&dir);
        assert!(store.write("../../etc/passwd", "bad", None).is_err());
        assert!(store.read("../../etc/passwd").is_err());
        assert!(store.delete("../../etc/passwd").is_err());
    }

    #[test]
    fn test_if_match_mismatch_conflict() {
        let dir = tempdir().unwrap();
        let store = make_store(&dir);
        store.write("flow", "content1", None).unwrap();
        let err = store
            .write("flow", "content2", Some("wrong-hash"))
            .unwrap_err();
        assert_eq!(err.code.as_str(), "ERR_CONFLICT");
    }

    #[test]
    fn test_delete_nonexistent_not_found() {
        let dir = tempdir().unwrap();
        let store = make_store(&dir);
        let err = store.delete("nonexistent").unwrap_err();
        assert_eq!(err.code.as_str(), "ERR_NOT_FOUND");
    }

    #[test]
    fn test_list_empty_dir() {
        let dir = tempdir().unwrap();
        // dir exists but no yaml files
        let store = make_store(&dir);
        let records = store.list().unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn test_list_ignores_non_yaml() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("readme.txt"), "not yaml").unwrap();
        std::fs::write(dir.path().join("flow.yaml"), "yaml content").unwrap();
        let store = make_store(&dir);
        let records = store.list().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "flow");
    }

    #[test]
    fn test_list_absent_dir_returns_empty() {
        let dir = tempdir().unwrap();
        let subdir = dir.path().join("workflows");
        let store = FsWorkflowFileStore::new(subdir);
        let records = store.list().unwrap();
        assert!(records.is_empty());
    }

    /// Mirrors `crate::fs_util::tests::unwritable_directory_returns_err`:
    /// this module's `atomic_write` wraps the shared helper's `io::Error`
    /// into this module's own `AppError` shape (spec 074, PR-3 / B1 + S1) —
    /// exercise that mapping directly rather than only via the higher-level
    /// workflow-file-persistence callers.
    #[test]
    #[cfg(unix)]
    fn atomic_write_unwritable_directory_returns_app_error() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let target = dir.path().join("flow.yaml");

        let mut perms = std::fs::metadata(dir.path()).unwrap().permissions();
        perms.set_mode(0o500); // r-x: no write permission.
        std::fs::set_permissions(dir.path(), perms).unwrap();

        let result = atomic_write(&target, b"payload");

        let mut restore = std::fs::metadata(dir.path()).unwrap().permissions();
        restore.set_mode(0o700);
        std::fs::set_permissions(dir.path(), restore).unwrap();

        let err = result.expect_err("writing into a read-only directory must fail");
        assert_eq!(err.category, ErrorCategory::IoError);
        assert!(
            err.message.contains("failed to atomically write"),
            "err={}",
            err.message
        );
    }
}
