#![allow(clippy::result_large_err)] // Git branch operations bubble AppError for command failures so extra boxing is unnecessary.

use crate::core::error::AppError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Manages git branch operations
pub struct BranchManager {
    workspace_path: PathBuf,
}

impl BranchManager {
    pub fn new(workspace_path: &Path) -> Self {
        Self {
            workspace_path: workspace_path.to_path_buf(),
        }
    }

    /// Create a new branch and check it out
    pub fn create_branch(&self, name: &str) -> Result<(), AppError> {
        let output = Command::new("git")
            .args(["checkout", "-b", name])
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
                    "Failed to create branch '{}': {}",
                    name,
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }

        Ok(())
    }

    /// Check out an existing branch
    pub fn checkout_branch(&self, name: &str) -> Result<(), AppError> {
        let output = Command::new("git")
            .args(["checkout", name])
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
                    "Failed to checkout branch '{}': {}",
                    name,
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }

        Ok(())
    }

    /// Check if a branch exists
    pub fn branch_exists(&self, name: &str) -> Result<bool, AppError> {
        let output = Command::new("git")
            .args(["show-ref", "--verify", &format!("refs/heads/{}", name)])
            .current_dir(&self.workspace_path)
            .output()
            .map_err(|e| {
                AppError::new(
                    crate::core::types::ErrorCategory::ToolExecutionError,
                    format!("Failed to execute git command: {}", e),
                )
            })?;

        Ok(output.status.success())
    }

    /// Generate a branch name from goal using branch_namer_cmd
    ///
    /// The command receives:
    /// - NEWTON_GOAL env var with the goal text
    /// - NEWTON_STATE_DIR env var with the state directory path
    ///
    /// The command can:
    /// - Write branch name to stdout (preferred)
    /// - Write branch name to state_dir/branch_name.txt (fallback)
    pub fn generate_branch_name(
        goal: &str,
        branch_namer_cmd: &str,
        state_dir: &Path,
    ) -> Result<String, AppError> {
        // Create state directory if it doesn't exist
        std::fs::create_dir_all(state_dir).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!(
                    "Failed to create state directory {}: {}",
                    state_dir.display(),
                    e
                ),
            )
        })?;

        // Set up environment variables
        let mut env_vars = HashMap::new();
        env_vars.insert("NEWTON_GOAL".to_string(), goal.to_string());
        env_vars.insert(
            "NEWTON_STATE_DIR".to_string(),
            state_dir.display().to_string(),
        );

        // Execute branch namer command
        let output = Command::new("sh")
            .arg("-c")
            .arg(branch_namer_cmd)
            .envs(&env_vars)
            .output()
            .map_err(|e| {
                AppError::new(
                    crate::core::types::ErrorCategory::ToolExecutionError,
                    format!("Failed to execute branch namer command: {}", e),
                )
            })?;

        if !output.status.success() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ToolExecutionError,
                format!(
                    "Branch namer command failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            ));
        }

        // Try reading from stdout first
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !stdout.is_empty() {
            return Ok(stdout);
        }

        // Fall back to reading from state_dir/branch_name.txt
        let branch_name_file = state_dir.join("branch_name.txt");
        if branch_name_file.exists() {
            let branch_name = std::fs::read_to_string(&branch_name_file)
                .map_err(|e| {
                    AppError::new(
                        crate::core::types::ErrorCategory::IoError,
                        format!(
                            "Failed to read branch name file {}: {}",
                            branch_name_file.display(),
                            e
                        ),
                    )
                })?
                .trim()
                .to_string();

            if !branch_name.is_empty() {
                return Ok(branch_name);
            }
        }

        Err(AppError::new(
            crate::core::types::ErrorCategory::ToolExecutionError,
            "Branch namer command produced no output (neither stdout nor branch_name.txt)",
        ))
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
        // Create initial commit
        std::fs::write(path.join("README.md"), "test").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(path)
            .output()
            .unwrap();
    }

    #[test]
    fn test_create_branch() {
        let temp_dir = TempDir::new().unwrap();
        init_git_repo(temp_dir.path());

        let manager = BranchManager::new(temp_dir.path());
        manager.create_branch("test-branch").unwrap();

        // Verify branch was created
        assert!(manager.branch_exists("test-branch").unwrap());
    }

    #[test]
    fn test_checkout_branch() {
        let temp_dir = TempDir::new().unwrap();
        init_git_repo(temp_dir.path());

        let manager = BranchManager::new(temp_dir.path());
        manager.create_branch("test-branch").unwrap();

        // Switch back to main/master
        let current = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        let main_branch = String::from_utf8_lossy(&current.stdout).trim().to_string();

        if main_branch != "test-branch" {
            manager.checkout_branch(&main_branch).unwrap();
        }

        // Now checkout test-branch
        manager.checkout_branch("test-branch").unwrap();

        // Verify we're on test-branch
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(branch, "test-branch");
    }

    #[test]
    fn test_branch_exists() {
        let temp_dir = TempDir::new().unwrap();
        init_git_repo(temp_dir.path());

        let manager = BranchManager::new(temp_dir.path());

        // Non-existent branch
        assert!(!manager.branch_exists("non-existent").unwrap());

        // Create and check
        manager.create_branch("test-branch").unwrap();
        assert!(manager.branch_exists("test-branch").unwrap());
    }

    #[test]
    fn test_generate_branch_name_stdout() {
        let temp_dir = TempDir::new().unwrap();
        let state_dir = temp_dir.path().join("state");

        let branch_name =
            BranchManager::generate_branch_name("Fix the bug", "echo feature/fix-bug", &state_dir)
                .unwrap();

        assert_eq!(branch_name, "feature/fix-bug");
    }

    #[test]
    fn test_generate_branch_name_file() {
        let temp_dir = TempDir::new().unwrap();
        let state_dir = temp_dir.path().join("state");

        // Command that writes to file instead of stdout
        let cmd = format!(
            "echo feature/from-file > {}",
            state_dir.join("branch_name.txt").display()
        );

        let branch_name =
            BranchManager::generate_branch_name("Fix the bug", &cmd, &state_dir).unwrap();

        assert_eq!(branch_name, "feature/from-file");
    }

    #[test]
    fn test_generate_branch_name_no_output() {
        let temp_dir = TempDir::new().unwrap();
        let state_dir = temp_dir.path().join("state");

        let result = BranchManager::generate_branch_name("Fix the bug", "true", &state_dir);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("produced no output"));
    }
}
