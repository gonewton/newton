use insta::assert_debug_snapshot;
use newton::core::config::ConfigLoader;
use serial_test::serial;
use std::env;
use std::fs;
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

/// Test integration of config loading with environment variables
#[test]
#[serial]
fn test_config_loading_integration() {
    clear_newton_env();
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path();

    // Create a comprehensive newton.toml
    let config_content = r#"
[project]
name = "integration-test"
template = "advanced-project"

[executor]
coding_agent = "advanced-agent"
coding_agent_model = "advanced-model"
auto_commit = true

[evaluator]
test_command = "./scripts/comprehensive-tests.sh"
score_threshold = 88.5

[context]
clear_after_use = false
file = ".custom/state/context.md"

[promise]
file = ".custom/state/promise.txt"
"#;

    fs::write(workspace_path.join("newton.toml"), config_content).unwrap();

    // Load config
    let config = ConfigLoader::load_from_workspace(workspace_path).unwrap();

    // Verify all sections are loaded correctly
    assert_debug_snapshot!(config);
    assert_eq!(config.project.name, "integration-test");
    assert_eq!(
        config.project.template,
        Some("advanced-project".to_string())
    );
    assert_eq!(config.executor.coding_agent, "advanced-agent");
    assert_eq!(config.executor.coding_agent_model, "advanced-model");
    assert!(config.executor.auto_commit);
    assert_eq!(
        config.evaluator.test_command,
        Some("./scripts/comprehensive-tests.sh".to_string())
    );
    assert_eq!(config.evaluator.score_threshold, 88.5);
    assert!(!config.context.clear_after_use);
    assert_eq!(
        config.context.file,
        std::path::PathBuf::from(".custom/state/context.md")
    );
    assert_eq!(
        config.promise.file,
        std::path::PathBuf::from(".custom/state/promise.txt")
    );
}

/// Test environment variable precedence over config file
#[test]
#[serial]
fn test_env_precedence_integration() {
    clear_newton_env();
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path();

    // Create initial config file
    let config_content = r#"
[project]
name = "file-project"
template = "file-template"

[executor]
coding_agent = "file-agent"
coding_agent_model = "file-model"
auto_commit = false

[evaluator]
test_command = "./file-tests.sh"
score_threshold = 75.0

[context]
clear_after_use = true
file = ".file/context.md"

[promise]
file = ".file/promise.txt"
"#;

    fs::write(workspace_path.join("newton.toml"), config_content).unwrap();

    // Set environment variables to override file values
    env::set_var("NEWTON_PROJECT_NAME", "env-project");
    env::set_var("NEWTON_PROJECT_TEMPLATE", "env-template");
    env::set_var("NEWTON_EXECUTOR_CODING_AGENT", "env-agent");
    env::set_var("NEWTON_EXECUTOR_CODING_AGENT_MODEL", "env-model");
    env::set_var("NEWTON_EXECUTOR_AUTO_COMMIT", "true");
    env::set_var("NEWTON_EVALUATOR_TEST_COMMAND", "./env-tests.sh");
    env::set_var("NEWTON_EVALUATOR_SCORE_THRESHOLD", "95.0");
    env::set_var("NEWTON_CONTEXT_CLEAR_AFTER_USE", "false");
    env::set_var("NEWTON_CONTEXT_FILE", ".env/context.md");
    env::set_var("NEWTON_PROMISE_FILE", ".env/promise.txt");

    // Load config (env vars should override file)
    let config = ConfigLoader::load_from_workspace(workspace_path).unwrap();

    assert_debug_snapshot!(config);

    // Verify environment variables take precedence
    assert_eq!(config.project.name, "env-project");
    assert_eq!(config.project.template, Some("env-template".to_string()));
    assert_eq!(config.executor.coding_agent, "env-agent");
    assert_eq!(config.executor.coding_agent_model, "env-model");
    assert!(config.executor.auto_commit);
    assert_eq!(
        config.evaluator.test_command,
        Some("./env-tests.sh".to_string())
    );
    assert_eq!(config.evaluator.score_threshold, 95.0);
    assert!(!config.context.clear_after_use);
    assert_eq!(
        config.context.file,
        std::path::PathBuf::from(".env/context.md")
    );
    assert_eq!(
        config.promise.file,
        std::path::PathBuf::from(".env/promise.txt")
    );

    // Clean up environment variables
    env::remove_var("NEWTON_PROJECT_NAME");
    env::remove_var("NEWTON_PROJECT_TEMPLATE");
    env::remove_var("NEWTON_EXECUTOR_CODING_AGENT");
    env::remove_var("NEWTON_EXECUTOR_CODING_AGENT_MODEL");
    env::remove_var("NEWTON_EXECUTOR_AUTO_COMMIT");
    env::remove_var("NEWTON_EVALUATOR_TEST_COMMAND");
    env::remove_var("NEWTON_EVALUATOR_SCORE_THRESHOLD");
    env::remove_var("NEWTON_CONTEXT_CLEAR_AFTER_USE");
    env::remove_var("NEWTON_CONTEXT_FILE");
    env::remove_var("NEWTON_PROMISE_FILE");
}

/// Test config loading without file (defaults + env vars)
#[test]
#[serial]
fn test_config_loading_without_file() {
    clear_newton_env();
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path();

    // Set only a few environment variables
    env::set_var("NEWTON_PROJECT_NAME", "no-file-project");
    env::set_var("NEWTON_EXECUTOR_AUTO_COMMIT", "true");

    // Load config (no file, some env vars)
    let config = ConfigLoader::load_from_workspace(workspace_path).unwrap();

    assert_debug_snapshot!(config);

    // Verify env vars are applied to defaults
    assert_eq!(config.project.name, "no-file-project");
    assert!(config.executor.auto_commit);

    // Verify defaults are used for unset values
    assert_eq!(config.executor.coding_agent, "opencode");
    assert_eq!(
        config.executor.coding_agent_model,
        "zai-coding-plan/glm-4.7"
    );
    assert_eq!(config.evaluator.score_threshold, 95.0);
    assert!(config.context.clear_after_use);

    // Clean up environment variables
    env::remove_var("NEWTON_PROJECT_NAME");
    env::remove_var("NEWTON_EXECUTOR_AUTO_COMMIT");
}

/// Test config validation in integration context
#[test]
#[serial]
fn test_config_validation_integration() {
    clear_newton_env();
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path();

    // Test valid config
    let valid_config = r#"
[project]
name = "valid-project"

[evaluator]
score_threshold = 85.0
"#;

    fs::write(workspace_path.join("newton.toml"), valid_config).unwrap();
    let config = ConfigLoader::load_from_workspace(workspace_path).unwrap();
    assert!(ConfigLoader::validate_config(&config).is_ok());

    // Test invalid config (empty project name)
    let invalid_config = r#"
[project]
name = ""
"#;

    fs::write(workspace_path.join("newton.toml"), invalid_config).unwrap();
    let config = ConfigLoader::load_from_workspace(workspace_path).unwrap();
    assert!(ConfigLoader::validate_config(&config).is_err());
}

/// Test config file path resolution
#[test]
#[serial]
fn test_config_file_path_resolution() {
    clear_newton_env();
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path();

    // Create config in workspace root
    let config_content = r#"
[project]
name = "path-test"
"#;

    fs::write(workspace_path.join("newton.toml"), config_content).unwrap();

    // Test loading from workspace root
    let config = ConfigLoader::load_from_workspace(workspace_path).unwrap();
    assert_eq!(config.project.name, "path-test");

    // Test direct file loading
    let config_path = workspace_path.join("newton.toml");
    let config_opt = ConfigLoader::load_from_file(&config_path).unwrap();
    assert!(config_opt.is_some());
    assert_eq!(config_opt.unwrap().project.name, "path-test");

    // Test loading non-existent file
    let non_existent_path = workspace_path.join("non-existent.toml");
    let config_opt = ConfigLoader::load_from_file(&non_existent_path).unwrap();
    assert!(config_opt.is_none());
}

/// Test partial configuration files
#[test]
#[serial]
fn test_partial_configuration() {
    clear_newton_env();
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path();

    // Test minimal config (only project name)
    let minimal_config = r#"
[project]
name = "minimal-test"
"#;

    fs::write(workspace_path.join("newton.toml"), minimal_config).unwrap();
    let config = ConfigLoader::load_from_workspace(workspace_path).unwrap();

    assert_debug_snapshot!(config);
    assert_eq!(config.project.name, "minimal-test");
    assert_eq!(config.executor.coding_agent, "opencode"); // Should use default
    assert_eq!(config.evaluator.score_threshold, 95.0); // Should use default

    // Test config with only some sections
    let partial_config = r#"
[project]
name = "partial-test"

[executor]
auto_commit = true

[context]
file = "custom/context.md"
"#;

    fs::write(workspace_path.join("newton.toml"), partial_config).unwrap();
    let config = ConfigLoader::load_from_workspace(workspace_path).unwrap();

    assert_eq!(config.project.name, "partial-test");
    assert_eq!(config.executor.coding_agent, "opencode"); // Default
    assert!(config.executor.auto_commit); // From file
    assert_eq!(
        config.context.file,
        std::path::PathBuf::from("custom/context.md")
    ); // From file
    assert!(config.context.clear_after_use); // Default
}
