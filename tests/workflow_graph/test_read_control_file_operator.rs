use newton::core::workflow_graph::operator::{ExecutionContext, Operator, StateView};
use newton::core::workflow_graph::operators::read_control_file::ReadControlFileOperator;
use serde_json::json;
use tempfile::tempdir;

fn execution_context(workspace: std::path::PathBuf) -> ExecutionContext {
    ExecutionContext {
        workspace_path: workspace,
        execution_id: "exec".to_string(),
        task_id: "read".to_string(),
        iteration: 1,
        state_view: StateView::new(json!({}), json!({}), json!({})),
    }
}

#[tokio::test]
async fn g6_missing_file_returns_done_false() {
    let workspace = tempdir().expect("workspace");
    let op = ReadControlFileOperator::new();
    let output = op
        .execute(
            json!({ "path": "missing.json" }),
            execution_context(workspace.path().to_path_buf()),
        )
        .await
        .expect("execute");
    assert_eq!(output["exists"], false);
    assert_eq!(output["done"], false);
}

#[tokio::test]
async fn g7_invalid_json_returns_wfg_ctrl_001() {
    let workspace = tempdir().expect("workspace");
    let control_file = workspace.path().join("control.json");
    std::fs::write(&control_file, "{ not json").expect("write");
    let op = ReadControlFileOperator::new();
    let err = op
        .execute(
            json!({ "path": control_file.display().to_string() }),
            execution_context(workspace.path().to_path_buf()),
        )
        .await
        .expect_err("invalid json");
    assert_eq!(err.code, "WFG-CTRL-001");
}
