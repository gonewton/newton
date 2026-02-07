#![allow(clippy::result_large_err)]

use super::NewtonConfig;
use crate::core::error::AppError;
use std::path::{Path, PathBuf};

pub struct ConfigLoader;

impl ConfigLoader {
    /// Load config from workspace root (workspace/newton.toml)
    /// Returns Ok(None) if file doesn't exist
    pub fn load_from_workspace(workspace_path: &Path) -> Result<Option<NewtonConfig>, AppError> {
        let config_path = workspace_path.join("newton.toml");
        Self::load_from_file(&config_path)
    }

    /// Load config from specific file path
    /// Returns Ok(None) if file doesn't exist
    pub fn load_from_file(path: &Path) -> Result<Option<NewtonConfig>, AppError> {
        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(path).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::IoError,
                format!("Failed to read config file {}: {}", path.display(), e),
            )
        })?;

        let config: NewtonConfig = toml::from_str(&content).map_err(|e| {
            AppError::new(
                crate::core::types::ErrorCategory::ValidationError,
                format!("Failed to parse config file {}: {}", path.display(), e),
            )
        })?;

        Ok(Some(config))
    }

    /// Merge config file with CLI arguments (CLI args take precedence)
    /// If config_file is None, returns a default config merged with CLI args
    #[allow(clippy::too_many_arguments)]
    pub fn merge_with_args(
        config_file: Option<NewtonConfig>,
        evaluator_cmd: Option<String>,
        advisor_cmd: Option<String>,
        executor_cmd: Option<String>,
        control_file: Option<PathBuf>,
        branch_from_goal: bool,
        restore_branch: bool,
        create_pr: bool,
    ) -> NewtonConfig {
        let mut config = config_file.unwrap_or_default();

        // CLI args override config file
        if evaluator_cmd.is_some() {
            config.evaluator_cmd = evaluator_cmd;
        }
        if advisor_cmd.is_some() {
            config.advisor_cmd = advisor_cmd;
        }
        if executor_cmd.is_some() {
            config.executor_cmd = executor_cmd;
        }
        if let Some(path) = control_file {
            config.control_file = path.display().to_string();
        }

        // Branch flags
        if branch_from_goal {
            config.branch.create_from_goal = true;
        }

        // Git flags
        if restore_branch {
            config.git.restore_original_branch = true;
        }
        if create_pr {
            config.git.create_pr_on_success = true;
        }

        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_config_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let result = ConfigLoader::load_from_workspace(temp_dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_load_config_valid() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("newton.toml");
        std::fs::write(
            &config_path,
            r#"
            evaluator_cmd = "eval.sh"
            control_file = "custom.json"
            "#,
        )
        .unwrap();

        let result = ConfigLoader::load_from_file(&config_path).unwrap();
        assert!(result.is_some());
        let config = result.unwrap();
        assert_eq!(config.evaluator_cmd, Some("eval.sh".to_string()));
        assert_eq!(config.control_file, "custom.json");
    }

    #[test]
    fn test_load_config_invalid() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("newton.toml");
        std::fs::write(&config_path, "invalid toml {{").unwrap();

        let result = ConfigLoader::load_from_file(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_cli_overrides_config() {
        let config = Some(NewtonConfig {
            evaluator_cmd: Some("config_eval.sh".to_string()),
            control_file: "config_control.json".to_string(),
            ..Default::default()
        });

        let merged = ConfigLoader::merge_with_args(
            config,
            Some("cli_eval.sh".to_string()),
            None,
            None,
            Some(PathBuf::from("cli_control.json")),
            false,
            false,
            false,
        );

        assert_eq!(merged.evaluator_cmd, Some("cli_eval.sh".to_string()));
        assert_eq!(merged.control_file, "cli_control.json");
    }

    #[test]
    fn test_merge_preserves_config_when_no_cli_args() {
        let config = Some(NewtonConfig {
            evaluator_cmd: Some("config_eval.sh".to_string()),
            advisor_cmd: Some("config_advisor.sh".to_string()),
            ..Default::default()
        });

        let merged =
            ConfigLoader::merge_with_args(config, None, None, None, None, false, false, false);

        assert_eq!(merged.evaluator_cmd, Some("config_eval.sh".to_string()));
        assert_eq!(merged.advisor_cmd, Some("config_advisor.sh".to_string()));
    }

    #[test]
    fn test_merge_branch_flags() {
        let config = Some(NewtonConfig::default());

        let merged = ConfigLoader::merge_with_args(
            config, None, None, None, None, true, // branch_from_goal
            true, // restore_branch
            true, // create_pr
        );

        assert!(merged.branch.create_from_goal);
        assert!(merged.git.restore_original_branch);
        assert!(merged.git.create_pr_on_success);
    }
}
