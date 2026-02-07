#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Control file structure written by evaluator to signal goal completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlFile {
    /// When true, Newton stops the optimization loop with success
    pub done: bool,

    /// Optional message from evaluator
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Optional metadata from evaluator
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Success policy that reads control file to determine if optimization should stop
pub struct SuccessPolicy {
    control_file_path: PathBuf,
}

impl SuccessPolicy {
    /// Create a new success policy
    ///
    /// # Arguments
    /// * `workspace_path` - The workspace root directory
    /// * `control_file_name` - The control file name (relative to workspace)
    pub fn new(workspace_path: &Path, control_file_name: &str) -> Self {
        let control_file_path = workspace_path.join(control_file_name);
        Self { control_file_path }
    }

    /// Check if optimization should stop
    ///
    /// Returns:
    /// - `Ok(true)` if control file exists and done = true
    /// - `Ok(false)` if control file doesn't exist or done = false
    /// - `Err(AppError)` if control file exists but is invalid JSON
    pub fn should_stop(&self) -> Result<bool, AppError> {
        // If file doesn't exist, continue (not an error)
        if !self.control_file_path.exists() {
            return Ok(false);
        }

        // Read and parse control file
        let content = std::fs::read_to_string(&self.control_file_path).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!(
                    "Failed to read control file {}: {}",
                    self.control_file_path.display(),
                    e
                ),
            )
        })?;

        let control: ControlFile = serde_json::from_str(&content).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::SerializationError,
                format!(
                    "Failed to parse control file {}: {}",
                    self.control_file_path.display(),
                    e
                ),
            )
        })?;

        Ok(control.done)
    }

    /// Read the full control file (if it exists)
    pub fn read_control_file(&self) -> Result<Option<ControlFile>, AppError> {
        if !self.control_file_path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&self.control_file_path).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!(
                    "Failed to read control file {}: {}",
                    self.control_file_path.display(),
                    e
                ),
            )
        })?;

        let control: ControlFile = serde_json::from_str(&content).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::SerializationError,
                format!(
                    "Failed to parse control file {}: {}",
                    self.control_file_path.display(),
                    e
                ),
            )
        })?;

        Ok(Some(control))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_should_stop_no_file() {
        let temp_dir = TempDir::new().unwrap();
        let policy = SuccessPolicy::new(temp_dir.path(), "newton_control.json");

        let result = policy.should_stop().unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_stop_done_true() {
        let temp_dir = TempDir::new().unwrap();
        let control_path = temp_dir.path().join("newton_control.json");
        std::fs::write(&control_path, r#"{"done": true}"#).unwrap();

        let policy = SuccessPolicy::new(temp_dir.path(), "newton_control.json");
        let result = policy.should_stop().unwrap();
        assert!(result);
    }

    #[test]
    fn test_should_stop_done_false() {
        let temp_dir = TempDir::new().unwrap();
        let control_path = temp_dir.path().join("newton_control.json");
        std::fs::write(&control_path, r#"{"done": false}"#).unwrap();

        let policy = SuccessPolicy::new(temp_dir.path(), "newton_control.json");
        let result = policy.should_stop().unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_stop_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        let control_path = temp_dir.path().join("newton_control.json");
        std::fs::write(&control_path, "invalid json {").unwrap();

        let policy = SuccessPolicy::new(temp_dir.path(), "newton_control.json");
        let result = policy.should_stop();
        assert!(result.is_err());
    }

    #[test]
    fn test_read_control_file_with_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let control_path = temp_dir.path().join("newton_control.json");
        std::fs::write(
            &control_path,
            r#"{
                "done": true,
                "message": "All tests passing",
                "metadata": {"test_count": 50}
            }"#,
        )
        .unwrap();

        let policy = SuccessPolicy::new(temp_dir.path(), "newton_control.json");
        let control = policy.read_control_file().unwrap().unwrap();

        assert!(control.done);
        assert_eq!(control.message, Some("All tests passing".to_string()));
        assert!(control.metadata.is_some());
    }

    #[test]
    fn test_read_control_file_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let policy = SuccessPolicy::new(temp_dir.path(), "newton_control.json");

        let result = policy.read_control_file().unwrap();
        assert!(result.is_none());
    }
}
