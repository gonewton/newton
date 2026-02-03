#![allow(clippy::result_large_err)]

use crate::core::entities::ArtifactMetadata;
use crate::core::error::AppError;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ArtifactStorageManager {
    root_path: PathBuf,
}

impl ArtifactStorageManager {
    pub fn new(root_path: PathBuf) -> Self {
        ArtifactStorageManager { root_path }
    }

    fn get_execution_path(&self, execution_id: &uuid::Uuid) -> PathBuf {
        self.root_path
            .join("artifacts")
            .join(execution_id.to_string())
    }

    pub fn get_artifact_path(&self, execution_id: &uuid::Uuid, artifact_id: uuid::Uuid) -> PathBuf {
        self.get_execution_path(execution_id)
            .join("artifacts")
            .join(artifact_id.to_string())
    }

    pub fn save_artifact(
        &self,
        path: &Path,
        content: &[u8],
        _metadata: ArtifactMetadata,
    ) -> Result<(), AppError> {
        if !path.exists() {
            let parent = path.parent().ok_or_else(|| {
                AppError::new(
                    crate::core::types::ErrorCategory::IoError,
                    "Invalid artifact path".to_string(),
                )
            })?;
            fs::create_dir_all(parent)?;
        }

        fs::write(path, content).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!("Failed to write artifact: {}", e),
            )
        })?;

        Ok(())
    }

    pub fn load_artifact(&self, path: &Path) -> Result<Vec<u8>, AppError> {
        fs::read(path).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!("Failed to read artifact: {}", e),
            )
        })
    }

    pub fn list_artifacts(
        &self,
        execution_id: &uuid::Uuid,
    ) -> Result<Vec<ArtifactMetadata>, AppError> {
        let execution_path = self.get_execution_path(execution_id);

        if !execution_path.exists() {
            return Ok(Vec::new());
        }

        let mut artifacts = Vec::new();

        if let Ok(entries) = fs::read_dir(execution_path) {
            for entry in entries.flatten() {
                let file_path = entry.path();

                if file_path.is_file() {
                    if let Ok(metadata) = fs::metadata(&file_path) {
                        let artifact_id = uuid::Uuid::new_v4();

                        artifacts.push(ArtifactMetadata {
                            id: artifact_id,
                            execution_id: Some(*execution_id),
                            iteration_id: None,
                            name: file_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("")
                                .to_string(),
                            path: file_path.clone(),
                            content_type: "application/octet-stream".to_string(),
                            size_bytes: metadata.len(),
                            created_at: metadata
                                .created()
                                .map(|t| {
                                    t.duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs() as i64
                                })
                                .unwrap_or(0),
                            modified_at: metadata
                                .modified()
                                .map(|t| {
                                    t.duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs() as i64
                                })
                                .unwrap_or(0),
                        });
                    }
                }
            }
        }

        Ok(artifacts)
    }

    pub fn delete_artifact(&self, artifact_id: uuid::Uuid) -> Result<(), AppError> {
        let artifact_path = self
            .root_path
            .join("artifacts")
            .join(artifact_id.to_string());

        if artifact_path.exists() {
            fs::remove_file(&artifact_path).map_err(|e| {
                AppError::new(
                    crate::core::types::ErrorCategory::IoError,
                    format!("Failed to delete artifact: {}", e),
                )
            })?;
        }

        Ok(())
    }

    pub fn get_artifact_metadata(
        &self,
        artifact_id: uuid::Uuid,
    ) -> Result<ArtifactMetadata, AppError> {
        let artifact_path = self
            .root_path
            .join("artifacts")
            .join(artifact_id.to_string());

        if !artifact_path.exists() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ArtifactError,
                "Artifact not found".to_string(),
            ));
        }

        let metadata = fs::metadata(&artifact_path).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!("Failed to read artifact metadata: {}", e),
            )
        })?;

        Ok(ArtifactMetadata {
            id: artifact_id,
            execution_id: None,
            iteration_id: None,
            name: artifact_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string(),
            path: artifact_path.clone(),
            content_type: "application/octet-stream".to_string(),
            size_bytes: metadata.len(),
            created_at: metadata
                .created()
                .map(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64
                })
                .unwrap_or(0),
            modified_at: metadata
                .modified()
                .map(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64
                })
                .unwrap_or(0),
        })
    }
}
