use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn set_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("chmod");
    }
}

fn write_script(path: &Path, body: &str) {
    fs::write(path, body).expect("write script");
    set_executable(path);
}

fn setup_workspace() -> (TempDir, PathBuf, PathBuf) {
    let workspace = TempDir::new().expect("workspace");
    let workspace_root = workspace.path().to_path_buf();
    let project_root = workspace_root.join("project-root");
    fs::create_dir_all(project_root.join(".newton").join("scripts")).expect("project scripts");
    fs::create_dir_all(workspace_root.join(".newton").join("scripts")).expect("workspace scripts");
    fs::create_dir_all(workspace_root.join(".newton").join("configs")).expect("configs");
    fs::create_dir_all(workspace_root.join(".newton").join("workflows")).expect("workflows");
    fs::create_dir_all(
        workspace_root
            .join(".newton")
            .join("plan")
            .join("proj")
            .join("todo"),
    )
    .expect("todo");
    fs::create_dir_all(
        workspace_root
            .join(".newton")
            .join("plan")
            .join("proj")
            .join("completed"),
    )
    .expect("completed");
    fs::create_dir_all(
        workspace_root
            .join(".newton")
            .join("plan")
            .join("proj")
            .join("failed"),
    )
    .expect("failed");

    write_script(
        &project_root
            .join(".newton")
            .join("scripts")
            .join("evaluator.sh"),
        r#"#!/bin/sh
set -e
printf "classic" > "$NEWTON_STATE_DIR/classic.marker"
printf '%s\n' '{"done":true}' > "$NEWTON_CONTROL_FILE"
"#,
    );
    write_script(
        &project_root
            .join(".newton")
            .join("scripts")
            .join("advisor.sh"),
        "#!/bin/sh\nexit 0\n",
    );
    write_script(
        &workspace_root
            .join(".newton")
            .join("scripts")
            .join("executor.sh"),
        "#!/bin/sh\nexit 0\n",
    );
    write_script(
        &workspace_root
            .join(".newton")
            .join("scripts")
            .join("coder.sh"),
        "#!/bin/sh\nexit 0\n",
    );

    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 3
    max_workflow_iterations: 10
    command_operator:
      allow_shell: true
  tasks:
    - id: start
      operator: CommandOperator
      params:
        shell: true
        cmd: 'printf "workflow" > "$NEWTON_STATE_DIR/workflow.marker"'
      terminal: success
"#;
    let workflow_file = workspace_root
        .join(".newton")
        .join("workflows")
        .join("wf.yaml");
    fs::write(&workflow_file, workflow).expect("workflow");
    (workspace, workspace_root, project_root)
}

#[test]
fn g2_conf_workflow_graph_executes_workflow_runner() {
    let (_temp, workspace_root, project_root) = setup_workspace();
    let config = r#"
project_root = ./project-root
coding_agent = test-agent
coding_model = test-model
runner = workflow_graph
workflow_file = .newton/workflows/wf.yaml
"#;
    fs::write(workspace_root.join(".newton/configs/proj.conf"), config).expect("config");
    fs::write(
        workspace_root.join(".newton/plan/proj/todo/item.plan"),
        "plain spec",
    )
    .expect("plan");

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("newton");
    cmd.arg("batch")
        .arg("proj")
        .arg("--workspace")
        .arg(&workspace_root)
        .arg("--once");
    cmd.assert().success();

    let tasks_root = project_root.join(".newton").join("tasks");
    let task_state = fs::read_dir(&tasks_root)
        .expect("tasks")
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("state"))
        .find(|path| path.is_dir())
        .expect("task state");
    assert!(task_state.join("workflow.marker").exists());
    assert!(!task_state.join("classic.marker").exists());
}

#[test]
fn g3_frontmatter_runner_override_wins_over_conf() {
    let (_temp, workspace_root, project_root) = setup_workspace();
    let config = r#"
project_root = ./project-root
coding_agent = test-agent
coding_model = test-model
runner = workflow_graph
workflow_file = .newton/workflows/wf.yaml
"#;
    fs::write(workspace_root.join(".newton/configs/proj.conf"), config).expect("config");
    fs::write(
        workspace_root.join(".newton/plan/proj/todo/item.plan"),
        r#"---
newton:
  runner: classic
---
spec
"#,
    )
    .expect("plan");

    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("newton");
    cmd.arg("batch")
        .arg("proj")
        .arg("--workspace")
        .arg(&workspace_root)
        .arg("--once");
    cmd.assert().success();

    let tasks_root = project_root.join(".newton").join("tasks");
    let task_state = fs::read_dir(&tasks_root)
        .expect("tasks")
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("state"))
        .find(|path| path.is_dir())
        .expect("task state");
    assert!(task_state.join("classic.marker").exists());
    assert!(!task_state.join("workflow.marker").exists());
}
