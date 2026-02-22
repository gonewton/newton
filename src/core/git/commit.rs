#![allow(clippy::result_large_err)] // Git commit helpers return AppError directly to preserve CLI diagnostics without boxing.

use crate::core::error::AppError;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Manages git commit and push operations
pub struct CommitManager {
    workspace_path: PathBuf,
}

impl CommitManager {
    pub fn new(workspace_path: &Path) -> Self {
        Self {
            workspace_path: workspace_path.to_path_buf(),
        }
    }

    /// Check if there are uncommitted changes
    pub fn has_changes(&self) -> Result<bool, AppError> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.workspace_path)
            .output()
            .map_err(|e| {
                AppError::new(
                    crate::core::types::ErrorCategory::ToolExecutionError,
                    format!("Failed to execute git command: {}", e),
                )
            })?;

        if !output.status.success() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ToolExecutionError,
                format!(
                    "Failed to check git status: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }

        Ok(!output.stdout.is_empty())
    }

    /// Commit all changes (git add -A && git commit)
    pub fn commit_all(&self, message: &str) -> Result<(), AppError> {
        // Check if there are changes to commit
        if !self.has_changes()? {
            // No changes, nothing to commit - this is not an error
            return Ok(());
        }

        // Stage all changes
        let add_output = Command::new("git")
            .args(["add", "-A"])
            .current_dir(&self.workspace_path)
            .output()
            .map_err(|e| {
                AppError::new(
                    crate::core::types::ErrorCategory::ToolExecutionError,
                    format!("Failed to execute git add: {}", e),
                )
            })?;

        if !add_output.status.success() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ToolExecutionError,
                format!(
                    "Failed to stage changes: {}",
                    String::from_utf8_lossy(&add_output.stderr)
                ),
            ));
        }

        // Commit changes
        let commit_output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&self.workspace_path)
            .output()
            .map_err(|e| {
                AppError::new(
                    crate::core::types::ErrorCategory::ToolExecutionError,
                    format!("Failed to execute git commit: {}", e),
                )
            })?;

        if !commit_output.status.success() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ToolExecutionError,
                format!(
                    "Failed to commit changes: {}",
                    String::from_utf8_lossy(&commit_output.stderr)
                ),
            ));
        }

        Ok(())
    }

    /// Push branch to remote (git push -u origin &lt;branch&gt;)
    pub fn push(&self, branch_name: &str) -> Result<(), AppError> {
        let output = Command::new("git")
            .args(["push", "-u", "origin", branch_name])
            .current_dir(&self.workspace_path)
            .output()
            .map_err(|e| {
                AppError::new(
                    crate::core::types::ErrorCategory::ToolExecutionError,
                    format!("Failed to execute git push: {}", e),
                )
            })?;

        if !output.status.success() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ToolExecutionError,
                format!(
                    "Failed to push branch '{}': {}",
                    branch_name,
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn init_git_repo(path: &Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .output()
            .unwrap();
    }

    #[test]
    fn test_has_changes_no_changes() {
        let temp_dir = TempDir::new().unwrap();
        init_git_repo(temp_dir.path());

        // Create initial commit
        std::fs::write(temp_dir.path().join("file.txt"), "content").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "Initial"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();

        let manager = CommitManager::new(temp_dir.path());
        assert!(!manager.has_changes().unwrap());
    }

    #[test]
    fn test_has_changes_with_changes() {
        let temp_dir = TempDir::new().unwrap();
        init_git_repo(temp_dir.path());

        // Create a file without committing
        std::fs::write(temp_dir.path().join("file.txt"), "content").unwrap();

        let manager = CommitManager::new(temp_dir.path());
        assert!(manager.has_changes().unwrap());
    }

    #[test]
    fn test_commit_all_with_changes() {
        let temp_dir = TempDir::new().unwrap();
        init_git_repo(temp_dir.path());

        // Create a file
        std::fs::write(temp_dir.path().join("file.txt"), "content").unwrap();

        let manager = CommitManager::new(temp_dir.path());
        manager.commit_all("Test commit").unwrap();

        // Verify no uncommitted changes
        assert!(!manager.has_changes().unwrap());
    }

    #[test]
    fn test_commit_all_no_changes() {
        let temp_dir = TempDir::new().unwrap();
        init_git_repo(temp_dir.path());

        // Create initial commit
        std::fs::write(temp_dir.path().join("file.txt"), "content").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "Initial"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();

        let manager = CommitManager::new(temp_dir.path());
        // Should not error when there are no changes
        manager.commit_all("No changes").unwrap();
    }
}
