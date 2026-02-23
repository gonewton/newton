use assert_cmd::Command;
use std::fs;
use std::path::Path;
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

fn setup_workspace() -> TempDir {
    let workspace = TempDir::new().expect("workspace");
    let root = workspace.path();
    let project_root = root.join("project-root");
    fs::create_dir_all(project_root.join(".newton").join("scripts")).expect("project scripts");
    fs::create_dir_all(root.join(".newton").join("scripts")).expect("workspace scripts");
    fs::create_dir_all(root.join(".newton").join("configs")).expect("configs");
    fs::create_dir_all(root.join(".newton").join("workflows")).expect("workflows");
    fs::create_dir_all(root.join(".newton").join("plan").join("proj").join("todo")).expect("todo");
    fs::create_dir_all(
        root.join(".newton")
            .join("plan")
            .join("proj")
            .join("completed"),
    )
    .expect("completed");
    fs::create_dir_all(
        root.join(".newton")
            .join("plan")
            .join("proj")
            .join("failed"),
    )
    .expect("failed");

    for script in ["advisor.sh", "evaluator.sh"] {
        let path = project_root.join(".newton").join("scripts").join(script);
        fs::write(&path, "#!/bin/sh\nexit 0\n").expect("script");
        set_executable(&path);
    }
    for script in ["executor.sh", "coder.sh"] {
        let path = root.join(".newton").join("scripts").join(script);
        fs::write(&path, "#!/bin/sh\nexit 0\n").expect("script");
        set_executable(&path);
    }

    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: run
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 3
    max_workflow_iterations: 10
    command_operator:
      allow_shell: true
    artifact_storage:
      base_path: ".newton/artifacts"
      max_inline_bytes: 1
      max_artifact_bytes: 104857600
      max_total_bytes: 1073741824
      retention_hours: 168
      cleanup_policy: lru
  tasks:
    - id: run
      operator: CommandOperator
      params:
        shell: true
        cmd: 'echo artifact-output'
      terminal: success
"#;
    fs::write(
        root.join(".newton").join("workflows").join("wf.yaml"),
        workflow,
    )
    .expect("workflow");
    fs::write(
        root.join(".newton").join("configs").join("proj.conf"),
        r#"
project_root = ./project-root
coding_agent = test-agent
coding_model = test-model
runner = workflow_graph
workflow_file = .newton/workflows/wf.yaml
"#,
    )
    .expect("config");
    fs::write(
        root.join(".newton")
            .join("plan")
            .join("proj")
            .join("todo")
            .join("item.plan"),
        "spec",
    )
    .expect("plan");
    workspace
}

#[test]
fn g5_workflow_batch_paths_are_under_task_state_dir() {
    let workspace = setup_workspace();
    let root = workspace.path();
    let mut cmd = Command::cargo_bin("newton").expect("bin");
    cmd.arg("batch")
        .arg("proj")
        .arg("--workspace")
        .arg(root)
        .arg("--once");
    cmd.assert().success();

    let tasks_root = root.join("project-root").join(".newton").join("tasks");
    let state_dir = fs::read_dir(&tasks_root)
        .expect("tasks root")
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("state"))
        .find(|path| path.is_dir())
        .expect("task state dir");

    let workflows_dir = state_dir.join("workflows");
    let workflow_run_dirs: Vec<_> = fs::read_dir(&workflows_dir)
        .expect("workflow state dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .collect();
    assert!(!workflow_run_dirs.is_empty());
    assert!(workflow_run_dirs[0].path().join("checkpoint.json").exists());
    assert!(workflow_run_dirs[0].path().join("execution.json").exists());

    let artifact_workflows = state_dir.join("artifacts").join("workflows");
    assert!(artifact_workflows.exists());
}
