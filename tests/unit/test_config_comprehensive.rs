use insta::assert_debug_snapshot;
use newton::core::config::{ConfigLoader, NewtonConfig};
use serial_test::serial;
use std::env;
use std::fs;
use tempfile::TempDir;

/// Comprehensive unit tests for configuration functionality
#[test]
fn test_config_serialization_roundtrip() {
    let original_config = NewtonConfig {
        project: newton::core::config::ProjectConfig {
            name: "test-project".to_string(),
            template: Some("test-template".to_string()),
        },
        executor: newton::core::config::ExecutorConfig {
            coding_agent: "test-agent".to_string(),
            coding_agent_model: "test-model".to_string(),
            auto_commit: true,
        },
        evaluator: newton::core::config::EvaluatorConfig {
            test_command: Some("./test.sh".to_string()),
            score_threshold: 85.0,
        },
        advisor: newton::core::config::AdvisorConfig::default(),
        context: newton::core::config::ContextConfig {
            clear_after_use: false,
            file: std::path::PathBuf::from("test/context.md"),
        },
        promise: newton::core::config::PromiseConfig {
            file: std::path::PathBuf::from("test/promise.txt"),
        },
    };

    // Serialize to TOML
    let toml_str = toml::to_string_pretty(&original_config).unwrap();

    // Deserialize back
    let deserialized: NewtonConfig = toml::from_str(&toml_str).unwrap();

    assert_debug_snapshot!(deserialized);
    assert_eq!(original_config.project.name, deserialized.project.name);
    assert_eq!(
        original_config.project.template,
        deserialized.project.template
    );
    assert_eq!(
        original_config.executor.coding_agent,
        deserialized.executor.coding_agent
    );
    assert_eq!(
        original_config.executor.auto_commit,
        deserialized.executor.auto_commit
    );
    assert_eq!(
        original_config.evaluator.score_threshold,
        deserialized.evaluator.score_threshold
    );
}

/// Test all default values
#[test]
fn test_all_default_values() {
    let config = NewtonConfig::default();

    assert_debug_snapshot!(config);

    // Project defaults
    assert_eq!(config.project.name, "newton-project");
    assert_eq!(config.project.template, None);

    // Executor defaults
    assert_eq!(config.executor.coding_agent, "opencode");
    assert_eq!(
        config.executor.coding_agent_model,
        "zai-coding-plan/glm-4.7"
    );
    assert!(!config.executor.auto_commit);

    // Evaluator defaults
    assert_eq!(config.evaluator.test_command, None);
    assert_eq!(config.evaluator.score_threshold, 95.0);

    // Context defaults
    assert!(config.context.clear_after_use);
    assert_eq!(
        config.context.file,
        std::path::PathBuf::from(".newton/state/context.md")
    );

    // Promise defaults
    assert_eq!(
        config.promise.file,
        std::path::PathBuf::from(".newton/state/promise.txt")
    );
}

/// Test environment variable parsing edge cases
#[test]
#[serial]
fn test_env_var_parsing_edge_cases() {
    env::remove_var("NEWTON_PROJECT_NAME");
    env::remove_var("NEWTON_EXECUTOR_AUTO_COMMIT");
    env::remove_var("NEWTON_EVALUATOR_SCORE_THRESHOLD");

    let temp_dir = TempDir::new().unwrap();

    // Test whitespace handling
    env::set_var("NEWTON_PROJECT_NAME", "  trimmed-name  ");
    env::set_var("NEWTON_EXECUTOR_AUTO_COMMIT", "  TRUE  ");
    env::set_var("NEWTON_EVALUATOR_SCORE_THRESHOLD", "  85.5  ");

    let config = ConfigLoader::load_from_workspace(temp_dir.path()).unwrap();

    // Should preserve whitespace for strings (user responsibility to trim)
    assert_eq!(config.project.name, "  trimmed-name  ");

    // Should parse boolean correctly even with whitespace
    assert!(config.executor.auto_commit);

    // Should parse float correctly even with whitespace
    assert_eq!(config.evaluator.score_threshold, 85.5);

    // Clean up
    env::remove_var("NEWTON_PROJECT_NAME");
    env::remove_var("NEWTON_EXECUTOR_AUTO_COMMIT");
    env::remove_var("NEWTON_EVALUATOR_SCORE_THRESHOLD");
}

/// Test malformed environment variables
#[test]
#[serial]
fn test_malformed_env_vars() {
    let temp_dir = TempDir::new().unwrap();

    // Test malformed boolean
    env::set_var("NEWTON_EXECUTOR_AUTO_COMMIT", "maybe");

    // Test malformed float
    env::set_var("NEWTON_EVALUATOR_SCORE_THRESHOLD", "not-a-number");

    let config = ConfigLoader::load_from_workspace(temp_dir.path()).unwrap();

    // Should fall back to defaults for malformed values
    assert!(!config.executor.auto_commit); // Default is false
    assert_eq!(config.evaluator.score_threshold, 95.0); // Default value

    // Clean up
    env::remove_var("NEWTON_EXECUTOR_AUTO_COMMIT");
    env::remove_var("NEWTON_EVALUATOR_SCORE_THRESHOLD");
}

/// Test config field ordering and serialization consistency
#[test]
#[serial]
fn test_config_consistency() {
    env::remove_var("NEWTON_PROJECT_NAME");
    env::remove_var("NEWTON_EXECUTOR_CODING_AGENT");
    env::remove_var("NEWTON_EXECUTOR_AUTO_COMMIT");
    let temp_dir = TempDir::new().unwrap();

    let config_content = r#"
[project]
name = "consistency-test"

[executor]
coding_agent = "test-agent"
auto_commit = true
"#;

    fs::write(temp_dir.path().join("newton.toml"), config_content).unwrap();

    // Load config multiple times to ensure consistency
    let config1 = ConfigLoader::load_from_workspace(temp_dir.path()).unwrap();
    let config2 = ConfigLoader::load_from_workspace(temp_dir.path()).unwrap();

    assert_eq!(config1.project.name, config2.project.name);
    assert_eq!(config1.executor.coding_agent, config2.executor.coding_agent);
    assert_eq!(config1.executor.auto_commit, config2.executor.auto_commit);

    // Serialize both and compare
    let toml1 = toml::to_string_pretty(&config1).unwrap();
    let toml2 = toml::to_string_pretty(&config2).unwrap();
    assert_eq!(toml1, toml2);
}

/// Test configuration with different file path formats
#[test]
fn test_file_path_formats() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path();

    // Test with absolute paths
    let abs_config_content = r#"
[project]
name = "abs-path-test"

[context]
file = "/absolute/path/context.md"

[promise]
file = "/absolute/path/promise.txt"
"#;

    fs::write(workspace_path.join("newton.toml"), abs_config_content).unwrap();
    let config = ConfigLoader::load_from_workspace(workspace_path).unwrap();

    assert_eq!(
        config.context.file,
        std::path::PathBuf::from("/absolute/path/context.md")
    );
    assert_eq!(
        config.promise.file,
        std::path::PathBuf::from("/absolute/path/promise.txt")
    );

    // Test with relative paths
    let rel_config_content = r#"
[project]
name = "rel-path-test"

[context]
file = "./relative/context.md"

[promise]
file = "relative/promise.txt"
"#;

    fs::write(workspace_path.join("newton.toml"), rel_config_content).unwrap();
    let config = ConfigLoader::load_from_workspace(workspace_path).unwrap();

    assert_eq!(
        config.context.file,
        std::path::PathBuf::from("./relative/context.md")
    );
    assert_eq!(
        config.promise.file,
        std::path::PathBuf::from("relative/promise.txt")
    );
}

/// Test configuration validation edge cases
#[test]
fn test_validation_edge_cases() {
    let mut config = NewtonConfig::default();

    // Test boundary values for score threshold
    config.evaluator.score_threshold = 0.0;
    assert!(ConfigLoader::validate_config(&config).is_ok());

    config.evaluator.score_threshold = 100.0;
    assert!(ConfigLoader::validate_config(&config).is_ok());

    config.evaluator.score_threshold = -0.1;
    assert!(ConfigLoader::validate_config(&config).is_err());

    config.evaluator.score_threshold = 100.1;
    assert!(ConfigLoader::validate_config(&config).is_err());

    // Test empty project name
    config.evaluator.score_threshold = 95.0; // Reset to valid
    config.project.name = "   ".to_string(); // Whitespace only
                                             // This should pass validation since it's not empty string (validation could be enhanced)
    assert!(ConfigLoader::validate_config(&config).is_ok());

    config.project.name = "".to_string();
    assert!(ConfigLoader::validate_config(&config).is_err());
}

/// Test version information is available in configuration context
#[test]
fn test_version_availability() {
    // This test ensures that version information is accessible
    // when configuration is loaded, which is required by the requirements

    let config = NewtonConfig::default();

    // The configuration should be loadable and valid
    assert!(ConfigLoader::validate_config(&config).is_ok());

    // Verify that we can access version through the package (from Cargo.toml at build time)
    let version = env!("CARGO_PKG_VERSION");
    assert!(!version.is_empty());
    // Semver-like (e.g. 0.5.11 or 0.5.12) so test does not break on version bumps or in CI release branches
    let parts: Vec<&str> = version.split('.').collect();
    assert!(
        parts.len() >= 2,
        "version should be semver-like, got {}",
        version
    );
    assert!(parts[0].parse::<u32>().is_ok(), "major should be numeric");
    assert!(parts[1].parse::<u32>().is_ok(), "minor should be numeric");
}
