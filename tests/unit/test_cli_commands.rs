use newton::cli::{commands, init, ErrorArgs, InitArgs, ReportArgs, RunArgs, StatusArgs, StepArgs};
use newton::core::entities::ExecutionConfiguration;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[tokio::test]
async fn test_run_command_success() {
    let temp_dir = TempDir::new().unwrap();
    let control_file = temp_dir.path().join("newton_control.json");
    fs::write(&control_file, r#"{"done": true}"#).unwrap();
    let args = RunArgs {
        path: temp_dir.path().to_path_buf(),
        max_iterations: 1,
        max_time: 60,
        evaluator_cmd: Some("echo 'test evaluator'".to_string()),
        advisor_cmd: Some("echo 'test advisor'".to_string()),
        executor_cmd: Some("echo 'test executor'".to_string()),
        evaluator_status_file: temp_dir.path().join("evaluator_status.md").clone(),
        advisor_recommendations_file: temp_dir.path().join("advisor_recommendations.md").clone(),
        executor_log_file: temp_dir.path().join("executor_log.md").clone(),
        tool_timeout_seconds: 30,
        evaluator_timeout: Some(5),
        advisor_timeout: Some(5),
        executor_timeout: Some(5),
        verbose: false,
        config: None,
        goal: None,
        goal_file: None,
        control_file: Some(control_file.clone()),
        feedback: None,
    };

    let result = commands::run(args).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_run_writes_goal_file_when_goal_text_provided() {
    let temp_dir = TempDir::new().unwrap();
    let control_file = temp_dir.path().join("newton_control.json");
    fs::write(&control_file, r#"{"done": true}"#).unwrap();
    let goal_text = "Ship version 1.0";
    let args = RunArgs {
        path: temp_dir.path().to_path_buf(),
        max_iterations: 1,
        max_time: 60,
        evaluator_cmd: Some("echo 'test evaluator'".to_string()),
        advisor_cmd: Some("echo 'test advisor'".to_string()),
        executor_cmd: Some("echo 'test executor'".to_string()),
        evaluator_status_file: temp_dir.path().join("evaluator_status.md").clone(),
        advisor_recommendations_file: temp_dir.path().join("advisor_recommendations.md").clone(),
        executor_log_file: temp_dir.path().join("executor_log.md").clone(),
        tool_timeout_seconds: 30,
        evaluator_timeout: Some(5),
        advisor_timeout: Some(5),
        executor_timeout: Some(5),
        verbose: false,
        config: None,
        goal: Some(goal_text.to_string()),
        goal_file: None,
        control_file: Some(control_file.clone()),
        feedback: None,
    };

    let result = commands::run(args).await;
    assert!(result.is_ok());

    let goal_path = temp_dir.path().join(".newton/state/goal.txt");
    let content = fs::read_to_string(&goal_path).unwrap();
    assert_eq!(content, goal_text);
}

#[tokio::test]
async fn test_step_command_basic() {
    let temp_dir = TempDir::new().unwrap();
    let args = StepArgs {
        path: temp_dir.path().to_path_buf(),
        execution_id: None,
        verbose: false,
    };

    let result = commands::step(args).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_status_command() {
    let temp_dir = TempDir::new().unwrap();
    let args = StatusArgs {
        execution_id: "test-execution-id".to_string(),
        path: temp_dir.path().to_path_buf(),
    };

    let result = commands::status(args).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_report_command() {
    let temp_dir = TempDir::new().unwrap();
    let args = ReportArgs {
        execution_id: "test-execution-id".to_string(),
        path: temp_dir.path().to_path_buf(),
        format: newton::cli::args::ReportFormat::Text,
    };

    let result = commands::report(args).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn test_error_command() {
    let _temp_dir = TempDir::new().unwrap();
    let args = ErrorArgs {
        execution_id: "test-execution-id".to_string(),
        verbose: false,
    };

    let result = commands::error(args).await;
    assert!(result.is_ok());
}

#[test]
fn test_execution_configuration_creation() {
    let config = ExecutionConfiguration {
        evaluator_cmd: Some("test cmd".to_string()),
        advisor_cmd: None,
        executor_cmd: None,
        max_time_seconds: Some(300),
        max_iterations: Some(10),
        evaluator_timeout_ms: Some(5000),
        advisor_timeout_ms: None,
        executor_timeout_ms: None,
        global_timeout_ms: Some(300000),
        strict_toolchain_mode: true,
        resource_monitoring: false,
        verbose: true,
    };

    assert_eq!(config.evaluator_cmd, Some("test cmd".to_string()));
    assert_eq!(config.max_time_seconds, Some(300));
    assert!(config.strict_toolchain_mode);
    assert!(config.verbose);
}

#[test]
fn test_run_args_defaults() {
    let args = RunArgs {
        path: PathBuf::from("/tmp"),
        max_iterations: 100,
        max_time: 3600,
        evaluator_cmd: None,
        advisor_cmd: None,
        executor_cmd: None,
        evaluator_status_file: PathBuf::new(),
        advisor_recommendations_file: PathBuf::new(),
        executor_log_file: PathBuf::new(),
        tool_timeout_seconds: 30,
        evaluator_timeout: None,
        advisor_timeout: None,
        executor_timeout: None,
        verbose: false,
        config: None,
        goal: None,
        goal_file: None,
        control_file: None,
        feedback: None,
    };

    assert_eq!(args.max_iterations, 100);
    assert_eq!(args.max_time, 3600);
    assert!(!args.verbose);
}

#[tokio::test]
async fn test_init_creates_workspace_structure() {
    let workspace = TempDir::new().unwrap();
    let template_source = create_aikit_template_fixture();

    let args = InitArgs {
        path: Some(workspace.path().to_path_buf()),
        template_source: Some(template_source.to_string_lossy().to_string()),
    };

    init::run(args).await.unwrap();

    let newton_dir = workspace.path().join(".newton");

    // Assert directory layout as per plan
    assert!(newton_dir.join("configs").is_dir());
    assert!(newton_dir.join("tasks").is_dir());
    assert!(newton_dir.join("plan/default/todo").is_dir());
    assert!(newton_dir.join("plan/default/completed").is_dir());
    assert!(newton_dir.join("plan/default/failed").is_dir());
    assert!(newton_dir.join("plan/default/draft").is_dir());
    assert!(newton_dir.join("state").is_dir());

    // Assert .newton/configs/default.conf exists and contains required fields
    let config_path = newton_dir.join("configs/default.conf");
    assert!(config_path.is_file());
    let config_content = fs::read_to_string(&config_path).unwrap();
    assert!(config_content.contains("project_root="));
    assert!(config_content.contains("coding_agent="));
    assert!(config_content.contains("coding_model="));

    // Assert scripts from template were installed
    assert!(newton_dir.join("scripts/advisor.sh").is_file());
    assert!(newton_dir.join("scripts/evaluator.sh").is_file());
    assert!(newton_dir.join("scripts/executor.sh").is_file()); // Either from template or stub
    assert!(newton_dir.join("README.md").is_file());
}

/// Create a minimal aikit template fixture for testing
fn create_aikit_template_fixture() -> PathBuf {
    let temp_dir = TempDir::new().unwrap();
    let template_dir = temp_dir.path().join("newton-template");
    fs::create_dir_all(template_dir.join("newton/scripts")).unwrap();

    // Create aikit.toml manifest
    let aikit_toml = r#"
[package]
name = "newton-template"
version = "1.0.0"
description = "Newton workspace template for testing"

[artifacts]
"newton/**" = ".newton"
"#;
    fs::write(template_dir.join("aikit.toml"), aikit_toml).unwrap();

    // Create template files
    fs::write(
        template_dir.join("newton/README.md"),
        "# Newton Workspace\n\nThis workspace was initialized with the Newton template.",
    )
    .unwrap();

    fs::write(
        template_dir.join("newton/scripts/advisor.sh"),
        "#!/bin/bash\necho 'advisor output'\n",
    )
    .unwrap();

    fs::write(
        template_dir.join("newton/scripts/evaluator.sh"),
        "#!/bin/bash\necho 'evaluator output'\n",
    )
    .unwrap();

    fs::write(
        template_dir.join("newton/scripts/executor.sh"),
        "#!/bin/bash\necho 'executor output'\n",
    )
    .unwrap();

    // Return the path and leak the TempDir so it's not cleaned up during the test
    let path = template_dir.clone();
    std::mem::forget(temp_dir);
    path
}
