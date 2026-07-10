#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::state::compute_sha256_hex;
use chrono::{DateTime, Utc};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug)]
pub struct WorkflowFileRecord {
    pub name: String,
    pub content: String,
    pub content_hash: String,
    pub size_bytes: u64,
    pub modified_at: DateTime<Utc>,
}

#[derive(Debug)]
pub enum WriteOutcome {
    /// Carries the persisted record — as read back from disk right after the
    /// write, in the same `write()` call — so API handlers echo the store's
    /// actual `content_hash`/`modified_at` instead of recomputing/guessing
    /// them (spec 074, B16).
    Created(WorkflowFileRecord),
    Updated(WorkflowFileRecord),
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
    /// Fix 6 (B17): cheap existence probe — `validate_slug` plus an fs
    /// `metadata()` stat, no content read and no SHA-256 hash computation.
    /// Callers that only need to know whether a name is taken (e.g. the
    /// `PUT` handler's no-precondition 428 check) should use this instead of
    /// `read()`, which pays for a full file read plus a hash it then
    /// discards.
    fn exists(&self, name: &str) -> Result<bool, AppError>;
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
        // `validate_slug` is the sole traversal defense for this write path:
        // it rejects `/`, `\`, and any `..` occurrence in `name` *before*
        // any joining happens, so `self.workflows_dir.join(format!("{name}.yaml"))`
        // below can only ever produce a direct child of `workflows_dir`.
        // (Previously there was also a post-join check comparing
        // `canonical_dir.join(name)` against `canonical_dir` — that was
        // tautologically true by construction and provided no real
        // protection; spec 074, B12.)
        validate_slug(name)?;
        // Ensure dir exists before path resolution
        fs::create_dir_all(&self.workflows_dir).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to create workflows dir: {e}"),
            )
        })?;
        let path = self.workflows_dir.join(format!("{name}.yaml"));

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
            } else {
                // Fix 5: `if_match` names a hash for a file state that no
                // longer exists — the file was deleted (or never existed)
                // since the caller last observed that hash. A precondition
                // referencing a vanished file state can never be satisfied,
                // so CAS must fail the same way a hash mismatch does,
                // rather than falling through to silently (re)create the
                // file as if this were an unconditional PUT.
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    "ETag mismatch: file does not exist (If-Match precondition cannot be \
                     satisfied against a deleted or never-created file)",
                )
                .with_code("ERR_CONFLICT"));
            }
        }
        atomic_write(&path, content.as_bytes())?;

        // Read back what was actually persisted (one extra `stat`, no extra
        // file read — the bytes are already in hand) so the caller echoes
        // the store's own numbers rather than the request's, per spec 074
        // B16. `content_hash` is deterministic over `content`'s bytes, but
        // `modified_at` must come from the filesystem: `Utc::now()` at the
        // handler layer can disagree with the mtime a subsequent GET
        // reports (clock vs. filesystem-timestamp granularity/skew).
        let metadata = fs::metadata(&path).map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to stat {}: {e}", path.display()),
            )
        })?;
        let record = WorkflowFileRecord {
            name: name.to_string(),
            content: content.to_string(),
            content_hash: compute_sha256_hex(content.as_bytes()),
            size_bytes: metadata.len(),
            modified_at: metadata
                .modified()
                .map(system_time_to_datetime)
                .unwrap_or_default(),
        };
        if existed {
            Ok(WriteOutcome::Updated(record))
        } else {
            Ok(WriteOutcome::Created(record))
        }
    }

    fn exists(&self, name: &str) -> Result<bool, AppError> {
        let path = resolve_and_check_path(&self.workflows_dir, name)?;
        Ok(fs::metadata(&path).is_ok())
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

    /// Spec 074, B12: the write-path traversal check
    /// (`target.starts_with(&canonical_dir)` on a `canonical_dir.join(...)`
    /// path) was tautologically true and has been deleted; `validate_slug`
    /// is now the sole, pre-join defense. These pin the specific traversal
    /// shapes called out by the fix — `../evil`, `a/b`, and absolute paths —
    /// on the write path directly (not just `resolve_and_check_path`'s
    /// read/delete path, already covered by `test_traversal_rejection`).
    #[test]
    fn test_write_rejects_dotdot_prefixed_name() {
        let dir = tempdir().unwrap();
        let store = make_store(&dir);
        let err = store.write("../evil", "bad", None).unwrap_err();
        assert_eq!(err.code.as_str(), "ERR_VALIDATION");
    }

    #[test]
    fn test_write_rejects_nested_slash_name() {
        let dir = tempdir().unwrap();
        let store = make_store(&dir);
        let err = store.write("a/b", "bad", None).unwrap_err();
        assert_eq!(err.code.as_str(), "ERR_VALIDATION");
    }

    #[test]
    fn test_write_rejects_absolute_unix_path_name() {
        let dir = tempdir().unwrap();
        let store = make_store(&dir);
        let err = store.write("/etc/passwd", "bad", None).unwrap_err();
        assert_eq!(err.code.as_str(), "ERR_VALIDATION");
    }

    #[test]
    fn test_write_rejects_absolute_windows_style_path_name() {
        let dir = tempdir().unwrap();
        let store = make_store(&dir);
        let err = store
            .write("C:\\Windows\\System32\\evil", "bad", None)
            .unwrap_err();
        assert_eq!(err.code.as_str(), "ERR_VALIDATION");
    }

    /// After rejecting a traversal-shaped write, the store must not have
    /// written anything outside `workflows_dir` — verifies the deleted dead
    /// check wasn't silently load-bearing.
    #[test]
    fn test_write_traversal_rejection_leaves_no_file_outside_store() {
        let dir = tempdir().unwrap();
        let store = make_store(&dir);
        assert!(store.write("../evil", "bad", None).is_err());
        let escaped = dir.path().parent().unwrap().join("evil.yaml");
        assert!(
            !escaped.exists(),
            "traversal write must not create a file outside workflows_dir"
        );
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

    /// Fix 5: `if_match: Some(_)` against a file that does not exist must
    /// fail CAS with the same conflict shape as a hash mismatch, not fall
    /// through to an unconditional create. Exercises the store layer
    /// directly (the API-level regression pin — GET/DELETE/PUT over HTTP —
    /// lives in `test_api::test_workflow_files_if_match_put_after_delete_conflicts_not_recreated`).
    #[test]
    fn test_if_match_against_nonexistent_file_conflicts_and_does_not_create() {
        let dir = tempdir().unwrap();
        let store = make_store(&dir);
        let err = store
            .write("never-existed", "content", Some("some-hash"))
            .unwrap_err();
        assert_eq!(err.code.as_str(), "ERR_CONFLICT");
        assert!(
            store.read("never-existed").is_err(),
            "conditional write against a nonexistent file must not create it"
        );
    }

    /// Same as above but for a file that existed and was deleted — proves
    /// the veto applies to "deleted since caller last observed it", not
    /// just "literally never existed".
    #[test]
    fn test_if_match_against_deleted_file_conflicts_and_does_not_recreate() {
        let dir = tempdir().unwrap();
        let store = make_store(&dir);
        store.write("flow", "content1", None).unwrap();
        let record = store.read("flow").unwrap();
        let old_hash = record.content_hash;
        store.delete("flow").unwrap();

        let err = store
            .write("flow", "content2", Some(&old_hash))
            .unwrap_err();
        assert_eq!(err.code.as_str(), "ERR_CONFLICT");
        assert!(
            store.read("flow").is_err(),
            "conditional write against a deleted file must not resurrect it"
        );
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
