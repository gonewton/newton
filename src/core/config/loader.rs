#![allow(clippy::result_large_err)] // Config loader returns AppError with rich context so we keep the concrete type instead of boxing.

use super::NewtonConfig;
use crate::core::error::AppError;
use std::env;
use std::path::{Path, PathBuf};

pub struct ConfigLoader;

impl ConfigLoader {
    /// Helper to parse boolean from environment variable with trimming
    fn parse_bool_env(value: &str) -> Option<bool> {
        value.trim().to_lowercase().parse::<bool>().ok()
    }

    /// Helper to parse f64 from environment variable with trimming
    fn parse_f64_env(value: &str) -> Option<f64> {
        value.trim().parse::<f64>().ok()
    }

    /// Load config from workspace root (workspace/newton.toml)
    /// Environment variables override config file values
    /// Returns default config if file doesn't exist (with env overrides applied)
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
        // Type alias to satisfy clippy::type_complexity
        type EnvOverride = (&'static str, fn(&str, &mut NewtonConfig));

        // Define override descriptors: (env_key, applier_function)
        let overrides: &[EnvOverride] = &[
            ("NEWTON_PROJECT_NAME", |val, cfg| {
                cfg.project.name = val.to_string();
            }),
            ("NEWTON_PROJECT_TEMPLATE", |val, cfg| {
                cfg.project.template = Some(val.to_string());
            }),
            ("NEWTON_EXECUTOR_CODING_AGENT", |val, cfg| {
                cfg.executor.coding_agent = val.to_string();
            }),
            ("NEWTON_EXECUTOR_CODING_AGENT_MODEL", |val, cfg| {
                cfg.executor.coding_agent_model = val.to_string();
            }),
            ("NEWTON_EXECUTOR_AUTO_COMMIT", |val, cfg| {
                if let Some(parsed) = Self::parse_bool_env(val) {
                    cfg.executor.auto_commit = parsed;
                }
            }),
            ("NEWTON_EVALUATOR_TEST_COMMAND", |val, cfg| {
                cfg.evaluator.test_command = Some(val.to_string());
            }),
            ("NEWTON_EVALUATOR_SCORE_THRESHOLD", |val, cfg| {
                if let Some(parsed) = Self::parse_f64_env(val) {
                    cfg.evaluator.score_threshold = parsed;
                }
            }),
            ("NEWTON_CONTEXT_CLEAR_AFTER_USE", |val, cfg| {
                if let Some(parsed) = Self::parse_bool_env(val) {
                    cfg.context.clear_after_use = parsed;
                }
            }),
            ("NEWTON_CONTEXT_FILE", |val, cfg| {
                cfg.context.file = PathBuf::from(val);
            }),
            ("NEWTON_PROMISE_FILE", |val, cfg| {
                cfg.promise.file = PathBuf::from(val);
            }),
        ];

        // Apply each override if the environment variable is set
        for (env_key, applier) in overrides {
            if let Ok(value) = env::var(env_key) {
                applier(&value, config);
            }
        }
    }

    /// Get documentation for supported environment variables
    pub fn env_var_documentation() -> &'static [&'static str] {
        &[
            "NEWTON_PROJECT_NAME - Override project name",
            "NEWTON_PROJECT_TEMPLATE - Override project template",
            "NEWTON_EXECUTOR_CODING_AGENT - Override executor coding agent (default: opencode)",
            "NEWTON_EXECUTOR_CODING_AGENT_MODEL - Override executor coding agent model (default: zai-coding-plan/glm-4.7)",
            "NEWTON_EXECUTOR_AUTO_COMMIT - Override auto commit setting (true/false, case-insensitive)",
            "NEWTON_EVALUATOR_TEST_COMMAND - Override evaluator test command",
            "NEWTON_EVALUATOR_SCORE_THRESHOLD - Override evaluator score threshold (default: 95.0)",
            "NEWTON_CONTEXT_CLEAR_AFTER_USE - Override context clear after use setting (true/false, default: true, case-insensitive)",
            "NEWTON_CONTEXT_FILE - Override context file path (default: .newton/state/context.md)",
            "NEWTON_PROMISE_FILE - Override promise file path (default: .newton/state/promise.txt)",
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

    fn assert_validate_error(config: &NewtonConfig, expected_substring: &str) {
        let result = ConfigLoader::validate_config(config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(expected_substring));
    }

    #[test]
    #[serial]
    fn test_load_config_nonexistent() {
        clear_newton_env();
        let temp_dir = TempDir::new().unwrap();
        let result = ConfigLoader::load_from_workspace(temp_dir.path()).unwrap();

        // Should return default config when file doesn't exist
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
        clear_newton_env();
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
        assert_debug_snapshot!(result);

        // Clean up environment variables
        env::remove_var("NEWTON_PROJECT_NAME");
        env::remove_var("NEWTON_EXECUTOR_CODING_AGENT");
        env::remove_var("NEWTON_EXECUTOR_AUTO_COMMIT");
        env::remove_var("NEWTON_EVALUATOR_SCORE_THRESHOLD");
    }

    #[test]
    #[serial]
    fn test_env_overrides_defaults() {
        clear_newton_env();
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

        assert_validate_error(&config, "Project name cannot be empty");
    }

    #[test]
    fn test_validate_config_invalid_score_threshold() {
        let mut config = NewtonConfig::default();
        config.evaluator.score_threshold = 150.0;

        assert_validate_error(&config, "Score threshold must be between 0.0 and 100.0");
    }

    #[test]
    fn test_env_var_documentation() {
        let docs = ConfigLoader::env_var_documentation();
        assert!(!docs.is_empty());
        assert!(docs.iter().any(|doc| doc.contains("NEWTON_PROJECT_NAME")));
        assert!(docs
            .iter()
            .any(|doc| doc.contains("NEWTON_EXECUTOR_CODING_AGENT")));
    }

    #[test]
    #[serial]
    fn test_invalid_env_var_values() {
        let temp_dir = TempDir::new().unwrap();

        // Set invalid boolean and float values
        env::set_var("NEWTON_EXECUTOR_AUTO_COMMIT", "invalid_bool");
        env::set_var("NEWTON_EVALUATOR_SCORE_THRESHOLD", "not-a-number");

        let result = ConfigLoader::load_from_workspace(temp_dir.path()).unwrap();

        // Should use default values when env vars are invalid
        assert!(!result.executor.auto_commit); // Default is false
        assert_eq!(result.evaluator.score_threshold, 95.0); // Default value

        // Clean up environment variables
        env::remove_var("NEWTON_EXECUTOR_AUTO_COMMIT");
        env::remove_var("NEWTON_EVALUATOR_SCORE_THRESHOLD");
    }

    #[test]
    fn test_parse_bool_env() {
        let test_cases = [
            ("true", Some(true)),
            ("TRUE", Some(true)),
            ("  true  ", Some(true)),
            ("  TRUE  ", Some(true)),
            ("false", Some(false)),
            ("FALSE", Some(false)),
            ("  false  ", Some(false)),
            ("invalid", None),
            ("", None),
        ];

        for (input, expected) in test_cases {
            assert_eq!(ConfigLoader::parse_bool_env(input), expected);
        }
    }

    #[test]
    fn test_parse_f64_env() {
        let test_cases = [
            ("85.5", Some(85.5)),
            ("  85.5  ", Some(85.5)),
            ("95.0", Some(95.0)),
            ("invalid", None),
            ("", None),
        ];

        for (input, expected) in test_cases {
            assert_eq!(ConfigLoader::parse_f64_env(input), expected);
        }
    }
}
