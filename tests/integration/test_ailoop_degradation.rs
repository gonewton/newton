use newton::cli::{args::RunArgs, commands};
use serial_test::serial;
use std::env;
use std::fs;
use tempfile::TempDir;

fn make_run_args(workspace: &std::path::Path, control_file: Option<std::path::PathBuf>) -> RunArgs {
    RunArgs {
        path: workspace.to_path_buf(),
        max_iterations: 1,
        max_time: 60,
        evaluator_cmd: Some("echo 'test evaluator'".to_string()),
        advisor_cmd: Some("echo 'test advisor'".to_string()),
        executor_cmd: Some("echo 'test executor'".to_string()),
        evaluator_status_file: workspace.join("evaluator_status.md"),
        advisor_recommendations_file: workspace.join("advisor_recommendations.md"),
        executor_log_file: workspace.join("executor_log.md"),
        tool_timeout_seconds: 30,
        evaluator_timeout: Some(5),
        advisor_timeout: Some(5),
        executor_timeout: Some(5),
        verbose: false,
        config: None,
        goal: None,
        goal_file: None,
        control_file,
        feedback: None,
    }
}

#[tokio::test]
#[serial]
async fn test_run_with_unreachable_ailoop_completes() {
    let temp_dir = TempDir::new().unwrap();
    let control_file = temp_dir.path().join("newton_control.json");
    fs::write(&control_file, r#"{"done": true}"#).unwrap();

    env::set_var("NEWTON_AILOOP_INTEGRATION", "1");
    env::set_var("NEWTON_AILOOP_HTTP_URL", "http://127.0.0.1:1");
    env::set_var("NEWTON_AILOOP_WS_URL", "ws://127.0.0.1:1");
    env::set_var("NEWTON_AILOOP_CHANNEL", "unreachable");

    let args = make_run_args(temp_dir.path(), Some(control_file.clone()));
    let result = commands::run(args).await;

    env::remove_var("NEWTON_AILOOP_INTEGRATION");
    env::remove_var("NEWTON_AILOOP_HTTP_URL");
    env::remove_var("NEWTON_AILOOP_WS_URL");
    env::remove_var("NEWTON_AILOOP_CHANNEL");

    assert!(result.is_ok(), "run should complete even if ailoop is unreachable");
}
