use newton_cli::cli::{args::RunArgs, commands};
use serial_test::serial;
use std::env;
use tempfile::TempDir;

fn make_run_args(workspace: &std::path::Path, workflow: &std::path::Path) -> RunArgs {
    RunArgs {
        workflow: workflow.to_path_buf(),
        input_file: None,
        workspace: Some(workspace.to_path_buf()),
        trigger: vec![],
        context: vec![],
        parameters_json: None,
        emit_completion_json: false,
        parallel_limit: None,
        timeout_seconds: Some(30),
        verbose: false,
        server: None,
        state_dir: None,
    }
}

#[tokio::test]
#[serial]
async fn test_run_with_unreachable_ailoop_completes() {
    let temp_dir = TempDir::new().unwrap();
    let workspace = temp_dir.path();
    std::fs::create_dir_all(workspace.join(".newton/state/workflows")).unwrap();

    let workflow_yaml = r#"version: "2.0"
mode: "workflow_graph"
metadata:
  name: "Ailoop degradation test"
workflow:
  settings:
    entry_task: "done_task"
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 1
    max_workflow_iterations: 10
  tasks:
    - id: "done_task"
      operator: "CommandOperator"
      params:
        cmd: "/bin/true"
      terminal: success
"#;
    let workflow_path = workspace.join("test_workflow.yaml");
    std::fs::write(&workflow_path, workflow_yaml).unwrap();

    env::set_var("NEWTON_AILOOP_INTEGRATION", "1");
    env::set_var("NEWTON_AILOOP_HTTP_URL", "http://127.0.0.1:1");
    env::set_var("NEWTON_AILOOP_WS_URL", "ws://127.0.0.1:1");
    env::set_var("NEWTON_AILOOP_CHANNEL", "unreachable");

    let args = make_run_args(workspace, &workflow_path);
    let result = commands::workflow_run(args).await.map_err(anyhow::Error::from);

    env::remove_var("NEWTON_AILOOP_INTEGRATION");
    env::remove_var("NEWTON_AILOOP_HTTP_URL");
    env::remove_var("NEWTON_AILOOP_WS_URL");
    env::remove_var("NEWTON_AILOOP_CHANNEL");

    assert!(result.is_ok(), "run should complete even if ailoop is unreachable: {:?}", result);
}
