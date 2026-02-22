#![allow(clippy::result_large_err)] // Pull request helpers keep AppError for detailed CLI failure reporting instead of boxing.

use crate::core::error::AppError;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Manages GitHub pull request operations via gh CLI
pub struct PullRequestManager {
    workspace_path: PathBuf,
}

impl PullRequestManager {
    pub fn new(workspace_path: &Path) -> Self {
        Self {
            workspace_path: workspace_path.to_path_buf(),
        }
    }

    /// Check if gh CLI is available
    pub fn is_gh_available(&self) -> bool {
        Command::new("gh")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Check if a PR exists for the given branch
    pub fn pr_exists(&self, branch_name: &str) -> Result<bool, AppError> {
        if !self.is_gh_available() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ToolExecutionError,
                "gh CLI is not installed or not available",
            ));
        }

        let output = Command::new("gh")
            .args(["pr", "list", "--head", branch_name, "--state", "open"])
            .current_dir(&self.workspace_path)
            .output()
            .map_err(|e| {
                AppError::new(
                    crate::core::types::ErrorCategory::ToolExecutionError,
                    format!("Failed to execute gh command: {}", e),
                )
            })?;

        if !output.status.success() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ToolExecutionError,
                format!(
                    "Failed to check for existing PR: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }

        // If output is not empty, a PR exists
        Ok(!output.stdout.is_empty())
    }

    /// Create a pull request
    ///
    /// # Arguments
    /// * `title` - PR title
    /// * `body` - PR description/body
    /// * `base` - Base branch (e.g., "main")
    pub fn create_pr(&self, title: &str, body: &str, base: &str) -> Result<String, AppError> {
        if !self.is_gh_available() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ToolExecutionError,
                "gh CLI is not installed or not available",
            ));
        }

        let output = Command::new("gh")
            .args([
                "pr", "create", "--title", title, "--body", body, "--base", base,
            ])
            .current_dir(&self.workspace_path)
            .output()
            .map_err(|e| {
                AppError::new(
                    crate::core::types::ErrorCategory::ToolExecutionError,
                    format!("Failed to execute gh command: {}", e),
                )
            })?;

        if !output.status.success() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ToolExecutionError,
                format!(
                    "Failed to create PR: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }

        // Return the PR URL from stdout
        let pr_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(pr_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_gh_available() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PullRequestManager::new(temp_dir.path());

        // This test will pass or fail depending on whether gh is installed
        // We just verify the method doesn't panic
        let _ = manager.is_gh_available();
    }

    #[test]
    fn test_pr_exists_without_gh() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PullRequestManager::new(temp_dir.path());

        // If gh is not available, should return error
        if !manager.is_gh_available() {
            let result = manager.pr_exists("test-branch");
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_create_pr_without_gh() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PullRequestManager::new(temp_dir.path());

        // If gh is not available, should return error
        if !manager.is_gh_available() {
            let result = manager.create_pr("Test PR", "Test body", "main");
            assert!(result.is_err());
        }
    }
}
