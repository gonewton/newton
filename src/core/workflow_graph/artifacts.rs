#![allow(clippy::result_large_err)] // Artifact store returns AppError to preserve structured diagnostic context; boxing would discard run-time state.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::checkpoint;
use crate::core::workflow_graph::schema::ArtifactStorageSettings;
use crate::core::workflow_graph::state::{compute_sha256_hex, validate_task_id, OutputRef};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

pub struct ArtifactStore {
    workspace_root: PathBuf,
    artifact_root: PathBuf,
    settings: ArtifactStorageSettings,
}

impl ArtifactStore {
    pub fn new(workspace_root: PathBuf, settings: &ArtifactStorageSettings) -> Self {
        let artifact_root = if settings.base_path.is_absolute() {
            settings.base_path.clone()
        } else {
            workspace_root.join(&settings.base_path)
        };
        ArtifactStore {
            workspace_root,
            artifact_root,
            settings: settings.clone(),
        }
    }

    pub fn route_output(
        &mut self,
        execution_id: &Uuid,
        task_id: &str,
        run_seq: usize,
        output: serde_json::Value,
    ) -> Result<OutputRef, AppError> {
        let serialized = serde_json::to_vec(&output)
            .map_err(|err| internal_serialization_error("output", err))?;
        let size = serialized.len() as u64;
        if size <= self.settings.max_inline_bytes as u64 {
            return Ok(OutputRef::Inline(output));
        }
        if size > self.settings.max_artifact_bytes as u64 {
            return Err(AppError::new(
                ErrorCategory::ArtifactError,
                "operator output exceeds max_artifact_bytes limit",
            )
            .with_code("WFG-ART-002"));
        }
        self.ensure_capacity(size)?;
        validate_task_id(task_id)?;
        let artifact_path = self
            .artifact_root
            .join("workflows")
            .join(execution_id.to_string())
            .join("task")
            .join(task_id)
            .join(run_seq.to_string())
            .join("output.json");
        if !artifact_path.starts_with(&self.artifact_root) {
            return Err(AppError::new(
                ErrorCategory::ArtifactError,
                "artifact path escapes base path",
            )
            .with_code("WFG-ART-001"));
        }
        let parent = artifact_path.parent().ok_or_else(|| {
            AppError::new(
                ErrorCategory::ArtifactError,
                "invalid artifact path for output",
            )
            .with_code("WFG-ART-001")
        })?;
        fs::create_dir_all(parent).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "failed to create artifact path {}: {}",
                    parent.display(),
                    err
                ),
            )
        })?;
        atomic_write(&artifact_path, &serialized)?;
        let sha256 = compute_sha256_hex(&serialized);
        let rel_path = artifact_path
            .strip_prefix(&self.workspace_root)
            .map_err(|_| {
                AppError::new(
                    ErrorCategory::ArtifactError,
                    "artifact path is outside workspace",
                )
            })?
            .to_path_buf();
        Ok(OutputRef::Artifact {
            path: rel_path,
            size_bytes: size,
            sha256,
        })
    }

    fn ensure_capacity(&mut self, upcoming: u64) -> Result<(), AppError> {
        let current = self.current_total_bytes()?;
        if current + upcoming <= self.settings.max_total_bytes {
            return Ok(());
        }
        let freed = self.cleanup(upcoming)?;
        if current + upcoming - freed <= self.settings.max_total_bytes {
            return Ok(());
        }
        Err(AppError::new(
            ErrorCategory::ArtifactError,
            "artifact store total size quota exceeded; cleanup could not free sufficient space",
        )
        .with_code("WFG-ART-003"))
    }

    fn current_total_bytes(&self) -> Result<u64, AppError> {
        let files = collect_artifact_files(&self.artifact_root)?;
        Ok(files.iter().map(|f| f.size).sum())
    }

    fn cleanup(&mut self, upcoming: u64) -> Result<u64, AppError> {
        let max_total = self.settings.max_total_bytes;
        let current = self.current_total_bytes()?;
        if current + upcoming <= max_total {
            return Ok(0);
        }
        let target = current + upcoming - max_total;
        let mut files = collect_artifact_files(&self.artifact_root)?;
        let retention = Duration::from_secs(self.settings.retention_hours * 3600);
        let live = checkpoint::collect_live_artifact_paths(&self.workspace_root, retention)?;
        files.sort_by_key(|f| f.modified);
        let mut freed = 0;
        for file in files {
            let canonical = file
                .path
                .canonicalize()
                .unwrap_or_else(|_| file.path.clone());
            if live.contains(&canonical) {
                continue;
            }
            if fs::remove_file(&file.path).is_ok() {
                freed += file.size;
            }
            if freed >= target {
                break;
            }
        }
        Ok(freed)
    }

    pub fn clean_artifacts(workspace_root: &Path, older_than: Duration) -> Result<(), AppError> {
        let settings = ArtifactStorageSettings::default();
        let store = ArtifactStore::new(workspace_root.to_path_buf(), &settings);
        let live = checkpoint::collect_live_artifact_paths(workspace_root, older_than)?;
        let files = collect_artifact_files(&store.artifact_root)?;
        for file in files {
            if live.contains(
                &file
                    .path
                    .canonicalize()
                    .unwrap_or_else(|_| file.path.clone()),
            ) {
                continue;
            }
            if SystemTime::now()
                .duration_since(file.modified)
                .unwrap_or_else(|_| Duration::from_secs(0))
                >= older_than
            {
                let _ = fs::remove_file(&file.path);
            }
        }
        Ok(())
    }
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to create {}: {}", parent.display(), err),
            )
        })?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data).map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to write {}: {}", tmp.display(), err),
        )
    })?;
    fs::rename(&tmp, path).map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!(
                "failed to rename {} -> {}: {}",
                tmp.display(),
                path.display(),
                err
            ),
        )
    })?;
    Ok(())
}

struct ArtifactFile {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

fn collect_artifact_files(base: &Path) -> Result<Vec<ArtifactFile>, AppError> {
    let mut files = Vec::new();
    if !base.exists() {
        return Ok(files);
    }
    fn recurse(dir: &Path, files: &mut Vec<ArtifactFile>) -> Result<(), AppError> {
        for entry in fs::read_dir(dir).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "failed to read artifact directory {}: {}",
                    dir.display(),
                    err
                ),
            )
        })? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                recurse(&path, files)?;
                continue;
            }
            let metadata = entry.metadata().map_err(|err| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to stat artifact {}: {}", path.display(), err),
                )
            })?;
            files.push(ArtifactFile {
                path,
                size: metadata.len(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            });
        }
        Ok(())
    }
    recurse(base, &mut files)?;
    Ok(files)
}

fn internal_serialization_error(target: &str, err: serde_json::Error) -> AppError {
    AppError::new(
        ErrorCategory::SerializationError,
        format!("failed to serialize {}: {}", target, err),
    )
}
