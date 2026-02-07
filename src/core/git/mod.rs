#![allow(clippy::result_large_err)]

mod branch;
mod commit;
mod pr;

pub use branch::BranchManager;
pub use commit::CommitManager;
pub use pr::PullRequestManager;

use crate::core::error::AppError;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Git operations manager - facade for git operations
pub struct GitManager {
    workspace_path: PathBuf,
}

impl GitManager {
    /// Create a new GitManager for the given workspace
    pub fn new(workspace_path: &Path) -> Self {
        Self {
            workspace_path: workspace_path.to_path_buf(),
        }
    }

    /// Check if the workspace is a git repository
    pub fn is_git_repo(&self) -> bool {
        self.workspace_path.join(".git").exists()
    }

    /// Get the current branch name
    pub fn current_branch(&self) -> Result<String, AppError> {
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("--abbrev-ref")
            .arg("HEAD")
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
                    "Failed to get current branch: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }

        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(branch)
    }

    /// Get a BranchManager instance
    pub fn branch_manager(&self) -> BranchManager {
        BranchManager::new(&self.workspace_path)
    }

    /// Get a CommitManager instance
    pub fn commit_manager(&self) -> CommitManager {
        CommitManager::new(&self.workspace_path)
    }

    /// Get a PullRequestManager instance
    pub fn pr_manager(&self) -> PullRequestManager {
        PullRequestManager::new(&self.workspace_path)
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
    fn test_is_git_repo() {
        let temp_dir = TempDir::new().unwrap();
        let git_manager = GitManager::new(temp_dir.path());

        // Not a git repo initially
        assert!(!git_manager.is_git_repo());

        // Initialize git repo
        init_git_repo(temp_dir.path());

        // Now it's a git repo
        assert!(git_manager.is_git_repo());
    }

    #[test]
    fn test_current_branch() {
        let temp_dir = TempDir::new().unwrap();
        init_git_repo(temp_dir.path());

        // Create an initial commit so HEAD exists
        std::fs::write(temp_dir.path().join("README.md"), "test").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();

        let git_manager = GitManager::new(temp_dir.path());
        let branch = git_manager.current_branch().unwrap();

        // Default branch is typically "main" or "master"
        assert!(branch == "main" || branch == "master");
    }
}
