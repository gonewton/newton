use newton::core::workflow_graph::{
    executor::{self, ExecutionOverrides},
    operator::OperatorRegistry,
    operators, schema, state,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::{tempdir, NamedTempFile};

const RESUME_WORKFLOW: &str = r#"
version: 2.0
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: first
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: first
      operator: NoOpOperator
      params: {}
      transitions:
        - to: second
          when:
            $expr: "true"
    - id: second
      operator: NoOpOperator
      params: {}
      transitions:
        - to: done
          when:
            $expr: "true"
    - id: done
      operator: NoOpOperator
      params: {}
"#;

const GOAL_GATE_GROUP_WORKFLOW: &str = r#"
version: 2.0
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: gate
    - id: gate
      operator: NoOpOperator
      params: {}
      goal_gate: true
      goal_gate_group: critical
"#;

fn build_registry(workspace: PathBuf, settings: state::GraphSettings) -> OperatorRegistry {
    let mut builder = OperatorRegistry::builder();
    operators::register_builtins(&mut builder, workspace, settings);
    builder.build()
}

fn write_workflow(yaml: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("temp file");
    write!(file, "{}", yaml).unwrap();
    file
}

fn read_json(path: &Path) -> Value {
    let bytes = fs::read(path).expect("read file");
    serde_json::from_slice(&bytes).expect("parse json")
}

fn write_json(path: &Path, value: &Value) {
    let bytes = serde_json::to_vec_pretty(value).expect("serialize json");
    fs::write(path, bytes).expect("write file");
}

#[tokio::test]
async fn resume_skips_completed_tasks() {
    let workspace = tempdir().expect("workspace");
    let workflow_file = write_workflow(RESUME_WORKFLOW);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings.clone());
    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
    };

    let summary = executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry.clone(),
        workspace.path().to_path_buf(),
        overrides,
    )
    .await
    .expect("first run succeeded");

    let state_dir = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(summary.execution_id.to_string());
    let execution_path = state_dir.join("execution.json");
    let checkpoint_path = state_dir.join("checkpoint.json");

    let mut execution_value = read_json(&execution_path);
    if let Some(array) = execution_value
        .get_mut("task_runs")
        .and_then(Value::as_array_mut)
    {
        let filtered: Vec<Value> = array
            .iter()
            .filter(|entry| entry["task_id"] == "first")
            .cloned()
            .collect();
        *array = filtered;
    }
    execution_value["status"] = Value::String("Running".to_string());
    execution_value["completed_at"] = Value::Null;
    write_json(&execution_path, &execution_value);

    let mut checkpoint_value = read_json(&checkpoint_path);
    if let Some(map) = checkpoint_value.as_object_mut() {
        map.insert("ready_queue".to_string(), json!(["second"]));
        map.insert("task_iterations".to_string(), json!({"first": 1}));
        map.insert("total_iterations".to_string(), json!(1));
        if let Some(completed) = map.get_mut("completed").and_then(Value::as_object_mut) {
            completed.retain(|key, _| key == "first");
        }
    }
    write_json(&checkpoint_path, &checkpoint_value);

    let resume_registry = build_registry(workspace.path().to_path_buf(), settings.clone());
    let resume_summary = executor::resume_workflow(
        resume_registry,
        workspace.path().to_path_buf(),
        summary.execution_id,
        false,
    )
    .await
    .expect("resume succeeded");

    assert!(resume_summary.total_iterations >= 3);
    let execution_value = read_json(&execution_path);
    let task_runs = execution_value["task_runs"]
        .as_array()
        .expect("task runs present");
    let ids: HashSet<_> = task_runs
        .iter()
        .map(|entry| entry["task_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains("first"));
    assert!(ids.contains("second"));
    assert!(ids.contains("done"));
    assert_eq!(
        execution_value["status"],
        Value::String("Completed".to_string())
    );

    let checkpoint_value = read_json(&checkpoint_path);
    let completed = checkpoint_value["completed"]
        .as_object()
        .expect("completed map");
    assert_eq!(completed.len(), 3);
}

#[tokio::test]
async fn resume_hash_mismatch_blocks_resume() {
    let workspace = tempdir().expect("workspace");
    let workflow_file = write_workflow(RESUME_WORKFLOW);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings.clone());
    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
    };

    let summary = executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry.clone(),
        workspace.path().to_path_buf(),
        overrides,
    )
    .await
    .expect("first run succeeded");

    let mut contents = fs::read_to_string(workflow_file.path()).expect("read workflow");
    contents.push('\n');
    fs::write(workflow_file.path(), contents).expect("rewrite workflow");

    let resume_registry = build_registry(workspace.path().to_path_buf(), settings.clone());
    let err = executor::resume_workflow(
        resume_registry,
        workspace.path().to_path_buf(),
        summary.execution_id,
        false,
    )
    .await
    .expect_err("hash mismatch should fail");
    assert_eq!(err.code, "WFG-CKPT-001");
}

#[tokio::test]
async fn checkpoint_records_goal_gate_group() {
    let workspace = tempdir().expect("workspace");
    let workflow_file = write_workflow(GOAL_GATE_GROUP_WORKFLOW);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings.clone());
    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
    };

    let summary = executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry,
        workspace.path().to_path_buf(),
        overrides,
    )
    .await
    .expect("workflow succeeded");

    let checkpoint_path = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(summary.execution_id.to_string())
        .join("checkpoint.json");

    let checkpoint_value = read_json(&checkpoint_path);
    assert_eq!(
        checkpoint_value["completed"]["gate"]["goal_gate_group"],
        Value::String("critical".to_string())
    );
}
