#![allow(clippy::result_large_err)]

use super::NewtonConfig;
use crate::core::error::AppError;
use std::env;
use std::path::{Path, PathBuf};

pub struct ConfigLoader;

impl ConfigLoader {
    /// Load config from workspace root (workspace/newton.toml)
    /// Environment variables override config file values
    /// Returns Ok(None) if file doesn't exist (will use defaults + env vars)
    pub fn load_from_workspace(workspace_path: &Path) -> Result<NewtonConfig, AppError> {
        let config_path = workspace_path.join("newton.toml");
        let config_file = Self::load_from_file(&config_path)?;

        let mut config = config_file.unwrap_or_default();

        // Apply environment variable overrides
        Self::apply_env_overrides(&mut config);

        Ok(config)
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

    /// Apply environment variable overrides to the configuration
    /// Environment variables take precedence over config file values
    fn apply_env_overrides(config: &mut NewtonConfig) {
        // Project overrides
        if let Ok(name) = env::var("NEWTON_PROJECT_NAME") {
            config.project.name = name;
        }

        if let Ok(template) = env::var("NEWTON_PROJECT_TEMPLATE") {
            config.project.template = Some(template);
        }

        // Executor overrides
        if let Ok(coding_agent) = env::var("NEWTON_EXECUTOR_CODING_AGENT") {
            config.executor.coding_agent = coding_agent;
        }

        if let Ok(coding_agent_model) = env::var("NEWTON_EXECUTOR_CODING_AGENT_MODEL") {
            config.executor.coding_agent_model = coding_agent_model;
        }

        if let Ok(auto_commit_str) = env::var("NEWTON_EXECUTOR_AUTO_COMMIT") {
            if let Ok(auto_commit) = auto_commit_str.parse::<bool>() {
                config.executor.auto_commit = auto_commit;
            }
        }

        // Evaluator overrides
        if let Ok(test_command) = env::var("NEWTON_EVALUATOR_TEST_COMMAND") {
            config.evaluator.test_command = Some(test_command);
        }

        if let Ok(score_threshold_str) = env::var("NEWTON_EVALUATOR_SCORE_THRESHOLD") {
            if let Ok(score_threshold) = score_threshold_str.parse::<f64>() {
                config.evaluator.score_threshold = score_threshold;
            }
        }

        // Context overrides
        if let Ok(clear_after_use_str) = env::var("NEWTON_CONTEXT_CLEAR_AFTER_USE") {
            if let Ok(clear_after_use) = clear_after_use_str.parse::<bool>() {
                config.context.clear_after_use = clear_after_use;
            }
        }

        if let Ok(context_file) = env::var("NEWTON_CONTEXT_FILE") {
            config.context.file = PathBuf::from(context_file);
        }

        // Promise overrides
        if let Ok(promise_file) = env::var("NEWTON_PROMISE_FILE") {
            config.promise.file = PathBuf::from(promise_file);
        }

        // Hooks overrides
        if let Ok(before_run) = env::var("NEWTON_HOOK_BEFORE_RUN") {
            config.hooks.before_run = Some(before_run);
        }

        if let Ok(after_run) = env::var("NEWTON_HOOK_AFTER_RUN") {
            config.hooks.after_run = Some(after_run);
        }
    }

    /// Get documentation for supported environment variables
    pub fn env_var_documentation() -> &'static [&'static str] {
        &[
            "NEWTON_PROJECT_NAME - Override project name",
            "NEWTON_PROJECT_TEMPLATE - Override project template",
        "NEWTON_EXECUTOR_CODING_AGENT - Override executor coding agent (default: opencode)",
        "NEWTON_EXECUTOR_CODING_AGENT_MODEL - Override executor coding agent model (default: zai-coding-plan/glm-4.7)",
        "NEWTON_EXECUTOR_AUTO_COMMIT - Override auto commit setting (true/false)",
        "NEWTON_EVALUATOR_TEST_COMMAND - Override evaluator test command",
        "NEWTON_EVALUATOR_SCORE_THRESHOLD - Override evaluator score threshold (default: 95.0)",
        "NEWTON_CONTEXT_CLEAR_AFTER_USE - Override context clear after use setting (true/false, default: true)",
        "NEWTON_CONTEXT_FILE - Override context file path (default: .newton/state/context.md)",
        "NEWTON_PROMISE_FILE - Override promise file path (default: .newton/state/promise.txt)",
        "NEWTON_HOOK_BEFORE_RUN - Override the before_run hook command",
        "NEWTON_HOOK_AFTER_RUN - Override the after_run hook command",
    ]
    }

    /// Validate configuration values
    pub fn validate_config(config: &NewtonConfig) -> Result<(), AppError> {
        // Validate project name is not empty
        if config.project.name.is_empty() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ValidationError,
                "Project name cannot be empty".to_string(),
            ));
        }

        // Validate score threshold is within reasonable bounds
        if config.evaluator.score_threshold < 0.0 || config.evaluator.score_threshold > 100.0 {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ValidationError,
                "Score threshold must be between 0.0 and 100.0".to_string(),
            ));
        }

        // Validate file paths are not empty
        if config.context.file.as_os_str().is_empty() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ValidationError,
                "Context file path cannot be empty".to_string(),
            ));
        }

        if config.promise.file.as_os_str().is_empty() {
            return Err(AppError::new(
                crate::core::types::ErrorCategory::ValidationError,
                "Promise file path cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;
    use serial_test::serial;
    use tempfile::TempDir;

    fn clear_newton_env() {
        for v in &[
            "NEWTON_PROJECT_NAME",
            "NEWTON_PROJECT_TEMPLATE",
            "NEWTON_EXECUTOR_CODING_AGENT",
            "NEWTON_EXECUTOR_CODING_AGENT_MODEL",
            "NEWTON_EXECUTOR_AUTO_COMMIT",
            "NEWTON_EVALUATOR_TEST_COMMAND",
            "NEWTON_EVALUATOR_SCORE_THRESHOLD",
            "NEWTON_CONTEXT_CLEAR_AFTER_USE",
            "NEWTON_CONTEXT_FILE",
            "NEWTON_PROMISE_FILE",
        ] {
            env::remove_var(v);
        }
    }

    #[test]
    #[serial]
    fn test_load_config_nonexistent() {
        clear_newton_env();
        let temp_dir = TempDir::new().unwrap();
        let result = ConfigLoader::load_from_workspace(temp_dir.path()).unwrap();
        assert_eq!(result.project.name, "newton-project");
        assert_eq!(result.executor.coding_agent, "opencode");
    }

    #[test]
    #[serial]
    fn test_load_config_valid() {
        clear_newton_env();
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("newton.toml");
        std::fs::write(
            &config_path,
            r#"
[project]
name = "test-project"
template = "test-template"

[executor]
coding_agent = "test-agent"
auto_commit = true

[evaluator]
test_command = "./test.sh"
score_threshold = 80.0
"#,
        )
        .unwrap();

        let result = ConfigLoader::load_from_workspace(temp_dir.path()).unwrap();
        assert_debug_snapshot!(result);

        assert_eq!(result.project.name, "test-project");
        assert_eq!(result.project.template, Some("test-template".to_string()));
        assert_eq!(result.executor.coding_agent, "test-agent");
        assert!(result.executor.auto_commit);
        assert_eq!(result.evaluator.test_command, Some("./test.sh".to_string()));
        assert_eq!(result.evaluator.score_threshold, 80.0);
    }

    #[test]
    #[serial]
    fn test_load_config_invalid() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("newton.toml");
        std::fs::write(&config_path, "invalid toml {{").unwrap();

        let result = ConfigLoader::load_from_workspace(temp_dir.path());
        assert!(result.is_err());
    }

    #[test]
    #[serial]
    fn test_env_overrides() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("newton.toml");
        std::fs::write(
            &config_path,
            r#"
[project]
name = "file-project"

[executor]
coding_agent = "file-agent"
auto_commit = false

[evaluator]
score_threshold = 75.0
"#,
        )
        .unwrap();

        // Set environment variables
        env::set_var("NEWTON_PROJECT_NAME", "env-project");
        env::set_var("NEWTON_EXECUTOR_CODING_AGENT", "env-agent");
        env::set_var("NEWTON_EXECUTOR_AUTO_COMMIT", "true");
        env::set_var("NEWTON_EVALUATOR_SCORE_THRESHOLD", "85.0");

        let result = ConfigLoader::load_from_workspace(temp_dir.path()).unwrap();

        // Environment variables should override file values
        assert_eq!(result.project.name, "env-project");
        assert_eq!(result.executor.coding_agent, "env-agent");
        assert!(result.executor.auto_commit);
        assert_eq!(result.evaluator.score_threshold, 85.0);

        // Clean up environment variables
        env::remove_var("NEWTON_PROJECT_NAME");
        env::remove_var("NEWTON_EXECUTOR_CODING_AGENT");
        env::remove_var("NEWTON_EXECUTOR_AUTO_COMMIT");
        env::remove_var("NEWTON_EVALUATOR_SCORE_THRESHOLD");
    }

    #[test]
    #[serial]
    fn test_env_overrides_defaults() {
        let temp_dir = TempDir::new().unwrap();

        // Set environment variables without config file
        env::set_var("NEWTON_PROJECT_NAME", "env-only-project");
        env::set_var("NEWTON_EXECUTOR_CODING_AGENT_MODEL", "env-model");

        let result = ConfigLoader::load_from_workspace(temp_dir.path()).unwrap();

        // Environment variables should override defaults
        assert_eq!(result.project.name, "env-only-project");
        assert_eq!(result.executor.coding_agent_model, "env-model");

        // Other values should use defaults
        assert_eq!(result.executor.coding_agent, "opencode");

        // Clean up environment variables
        env::remove_var("NEWTON_PROJECT_NAME");
        env::remove_var("NEWTON_EXECUTOR_CODING_AGENT_MODEL");
    }

    #[test]
    fn test_validate_config_success() {
        let config = NewtonConfig::default();
        assert!(ConfigLoader::validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_empty_project_name() {
        let mut config = NewtonConfig::default();
        config.project.name = "".to_string();

        let result = ConfigLoader::validate_config(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Project name cannot be empty"));
    }

    #[test]
    fn test_validate_config_invalid_score_threshold() {
        let mut config = NewtonConfig::default();
        config.evaluator.score_threshold = 150.0;

        let result = ConfigLoader::validate_config(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Score threshold must be between 0.0 and 100.0"));
    }

    #[test]
    fn test_env_var_documentation() {
        let docs = ConfigLoader::env_var_documentation();
        assert!(!docs.is_empty());
        assert!(docs.iter().any(|doc| doc.contains("NEWTON_PROJECT_NAME")));
        assert!(docs
            .iter()
            .any(|doc| doc.contains("NEWTON_EXECUTOR_CODING_AGENT")));
        assert!(docs
            .iter()
            .any(|doc| doc.contains("NEWTON_HOOK_BEFORE_RUN")));
        assert!(docs.iter().any(|doc| doc.contains("NEWTON_HOOK_AFTER_RUN")));
    }

    #[test]
    #[serial]
    fn test_invalid_env_var_values() {
        let temp_dir = TempDir::new().unwrap();

        // Set invalid boolean and float values
        env::set_var("NEWTON_EXECUTOR_AUTO_COMMIT", "invalid_bool");
        env::set_var("NEWTON_EVALUATOR_SCORE_THRESHOLD", "invalid_float");

        let result = ConfigLoader::load_from_workspace(temp_dir.path()).unwrap();

        // Should use default values when env vars are invalid
        assert!(!result.executor.auto_commit); // Default is false
        assert_eq!(result.evaluator.score_threshold, 95.0); // Default value

        // Clean up environment variables
        env::remove_var("NEWTON_EXECUTOR_AUTO_COMMIT");
        env::remove_var("NEWTON_EVALUATOR_SCORE_THRESHOLD");
    }
}
