/// Integration tests for resume with allow_workflow_change settings merge.
///
/// Tests verify that:
/// 1. Resume with allow_workflow_change=true updates iteration limits from current document
/// 2. Resume without allow_workflow_change=false preserves checkpoint settings
/// 3. Other settings (parallel_limit, max_time_seconds, continue_on_error) are preserved
use newton_core::workflow::{
    executor::{self, ExecutionOverrides},
    operator::OperatorRegistry,
    operators, schema, state,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tempfile::{tempdir, NamedTempFile};

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

fn read_json(path: &std::path::Path) -> Value {
    let bytes = fs::read(path).expect("read file");
    serde_json::from_slice(&bytes).expect("parse json")
}

fn write_json(path: &std::path::Path, value: &Value) {
    let bytes = serde_json::to_vec_pretty(value).expect("serialize json");
    fs::write(path, bytes).expect("write file");
}

fn default_overrides() -> ExecutionOverrides {
    ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        server_notifier: None,
        pre_seed_nodes: true,
    }
}

#[tokio::test]
async fn resume_with_allow_workflow_change_uses_updated_iteration_limits() {
    let workspace = tempdir().expect("workspace");

    let workflow_low_limits = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: task1
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 3
    max_workflow_iterations: 5
  tasks:
    - id: task1
      operator: NoOpOperator
      params: {}
      transitions:
        - to: task2
    - id: task2
      operator: NoOpOperator
      params: {}
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      params: {}
"#;

    let workflow_file = write_workflow(workflow_low_limits);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings.clone());

    let summary = executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry.clone(),
        workspace.path().to_path_buf(),
        default_overrides(),
    )
    .await
    .expect("initial execution succeeded");

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
            .filter(|entry| entry["task_id"] == "task1")
            .cloned()
            .collect();
        *array = filtered;
    }
    execution_value["status"] = Value::String("Running".to_string());
    execution_value["completed_at"] = Value::Null;
    write_json(&execution_path, &execution_value);

    let mut checkpoint_value = read_json(&checkpoint_path);
    if let Some(map) = checkpoint_value.as_object_mut() {
        map.insert("ready_queue".to_string(), json!(["task2"]));
        map.insert("task_iterations".to_string(), json!({"task1": 1}));
        map.insert("total_iterations".to_string(), json!(1));
        if let Some(completed) = map.get_mut("completed").and_then(Value::as_object_mut) {
            completed.retain(|key, _| key == "task1");
        }
    }
    write_json(&checkpoint_path, &checkpoint_value);

    let workflow_higher_limits = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: task1
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 50
    max_workflow_iterations: 100
  tasks:
    - id: task1
      operator: NoOpOperator
      params: {}
      transitions:
        - to: task2
    - id: task2
      operator: NoOpOperator
      params: {}
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      params: {}
"#;
    fs::write(workflow_file.path(), workflow_higher_limits).expect("update workflow");

    let resume_registry = build_registry(workspace.path().to_path_buf(), settings);
    let resume_summary = executor::resume_workflow(
        resume_registry,
        workspace.path().to_path_buf(),
        summary.execution_id,
        true,
    )
    .await
    .expect("resume with allow_workflow_change succeeded");

    assert!(resume_summary.total_iterations >= 3);

    let final_execution = read_json(&execution_path);
    let task_runs = final_execution["task_runs"]
        .as_array()
        .expect("task runs present");
    let ids: HashSet<_> = task_runs
        .iter()
        .map(|entry| entry["task_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains("task1"));
    assert!(ids.contains("task2"));
    assert!(ids.contains("done"));
}

#[tokio::test]
async fn resume_preserves_non_iteration_settings_from_checkpoint() {
    let workspace = tempdir().expect("workspace");

    let workflow_original = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: task1
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: task1
      operator: NoOpOperator
      params: {}
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      params: {}
"#;

    let workflow_file = write_workflow(workflow_original);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings.clone());

    let summary = executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry.clone(),
        workspace.path().to_path_buf(),
        default_overrides(),
    )
    .await
    .expect("initial execution succeeded");

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
            .filter(|entry| entry["task_id"] == "task1")
            .cloned()
            .collect();
        *array = filtered;
    }
    execution_value["status"] = Value::String("Running".to_string());
    execution_value["completed_at"] = Value::Null;
    write_json(&execution_path, &execution_value);

    let mut checkpoint_value = read_json(&checkpoint_path);
    if let Some(map) = checkpoint_value.as_object_mut() {
        map.insert("ready_queue".to_string(), json!(["done"]));
        map.insert("task_iterations".to_string(), json!({"task1": 1}));
        map.insert("total_iterations".to_string(), json!(1));
        if let Some(completed) = map.get_mut("completed").and_then(Value::as_object_mut) {
            completed.retain(|key, _| key == "task1");
        }
    }
    write_json(&checkpoint_path, &checkpoint_value);

    let workflow_changed_settings = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: task1
    max_time_seconds: 999
    parallel_limit: 999
    continue_on_error: true
    max_task_iterations: 100
    max_workflow_iterations: 200
  tasks:
    - id: task1
      operator: NoOpOperator
      params: {}
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      params: {}
"#;
    fs::write(workflow_file.path(), workflow_changed_settings).expect("update workflow");

    let resume_registry = build_registry(workspace.path().to_path_buf(), settings);
    let _resume_summary = executor::resume_workflow(
        resume_registry,
        workspace.path().to_path_buf(),
        summary.execution_id,
        true,
    )
    .await
    .expect("resume succeeded");
}

#[tokio::test]
async fn resume_without_allow_workflow_change_uses_checkpoint_limits() {
    let workspace = tempdir().expect("workspace");

    let workflow_original = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: task1
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: task1
      operator: NoOpOperator
      params: {}
      transitions:
        - to: task2
    - id: task2
      operator: NoOpOperator
      params: {}
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      params: {}
"#;

    let workflow_file = write_workflow(workflow_original);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings.clone());

    let summary = executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry.clone(),
        workspace.path().to_path_buf(),
        default_overrides(),
    )
    .await
    .expect("initial execution succeeded");

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
            .filter(|entry| entry["task_id"] == "task1")
            .cloned()
            .collect();
        *array = filtered;
    }
    execution_value["status"] = Value::String("Running".to_string());
    execution_value["completed_at"] = Value::Null;
    write_json(&execution_path, &execution_value);

    let mut checkpoint_value = read_json(&checkpoint_path);
    if let Some(map) = checkpoint_value.as_object_mut() {
        map.insert("ready_queue".to_string(), json!(["task2"]));
        map.insert("task_iterations".to_string(), json!({"task1": 1}));
        map.insert("total_iterations".to_string(), json!(1));
        if let Some(completed) = map.get_mut("completed").and_then(Value::as_object_mut) {
            completed.retain(|key, _| key == "task1");
        }
    }
    write_json(&checkpoint_path, &checkpoint_value);

    let resume_registry = build_registry(workspace.path().to_path_buf(), settings);
    let resume_summary = executor::resume_workflow(
        resume_registry,
        workspace.path().to_path_buf(),
        summary.execution_id,
        false,
    )
    .await
    .expect("resume without allow_workflow_change succeeded");

    assert!(resume_summary.total_iterations >= 3);

    let final_execution = read_json(&execution_path);
    let settings_effective = &final_execution["settings_effective"];

    assert_eq!(settings_effective["max_time_seconds"], json!(60));
    assert_eq!(settings_effective["parallel_limit"], json!(1));
    assert_eq!(settings_effective["continue_on_error"], json!(false));
    assert_eq!(settings_effective["max_task_iterations"], json!(5));
    assert_eq!(settings_effective["max_workflow_iterations"], json!(10));
}

#[tokio::test]
async fn resume_inconsistent_checkpoint_returns_wfg_resume_002() {
    // When a checkpoint has empty ready_queue but total_iterations > completed.len()
    // (old-format aborted checkpoint), resume must return WFG-RESUME-002.
    let workspace = tempdir().expect("workspace");

    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: task1
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: task1
      operator: NoOpOperator
      params: {}
      transitions:
        - to: task2
    - id: task2
      operator: NoOpOperator
      params: {}
"#;

    let workflow_file = write_workflow(workflow);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings.clone());

    let summary = executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry.clone(),
        workspace.path().to_path_buf(),
        default_overrides(),
    )
    .await
    .expect("initial execution succeeded");

    let state_dir = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(summary.execution_id.to_string());
    let execution_path = state_dir.join("execution.json");
    let checkpoint_path = state_dir.join("checkpoint.json");

    // Simulate an old-format checkpoint: ready_queue is empty but
    // total_iterations (2) exceeds completed.len() (1), as if a task
    // hard-aborted without being re-queued.
    let mut execution_value = read_json(&execution_path);
    execution_value["status"] = json!("Running");
    execution_value["completed_at"] = json!(null);
    write_json(&execution_path, &execution_value);

    let mut checkpoint_value = read_json(&checkpoint_path);
    if let Some(map) = checkpoint_value.as_object_mut() {
        map.insert("ready_queue".to_string(), json!([]));
        map.insert(
            "task_iterations".to_string(),
            json!({"task1": 1, "task2": 1}),
        );
        map.insert("total_iterations".to_string(), json!(2));
        // Only task1 completed — task2 aborted without being re-queued (old format).
        if let Some(completed) = map.get_mut("completed").and_then(Value::as_object_mut) {
            completed.retain(|key, _| key == "task1");
        }
    }
    write_json(&checkpoint_path, &checkpoint_value);

    let resume_registry = build_registry(workspace.path().to_path_buf(), settings.clone());
    let err = executor::resume_workflow(
        resume_registry,
        workspace.path().to_path_buf(),
        summary.execution_id,
        false,
    )
    .await
    .expect_err("resume of inconsistent checkpoint must fail");

    assert_eq!(err.code, "WFG-RESUME-002");
    assert!(
        err.message.contains("2 tasks ran"),
        "error message should contain task count: {}",
        err.message
    );
    assert!(
        err.message.contains("1 completed"),
        "error message should contain completed count: {}",
        err.message
    );
}

#[tokio::test]
async fn resume_inconsistent_checkpoint_with_allow_workflow_change_also_returns_wfg_resume_002() {
    // The WFG-RESUME-002 guard fires regardless of allow_workflow_change.
    let workspace = tempdir().expect("workspace");

    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: task1
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: task1
      operator: NoOpOperator
      params: {}
      transitions:
        - to: task2
    - id: task2
      operator: NoOpOperator
      params: {}
"#;

    let workflow_file = write_workflow(workflow);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings.clone());

    let summary = executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry.clone(),
        workspace.path().to_path_buf(),
        default_overrides(),
    )
    .await
    .expect("initial execution succeeded");

    let state_dir = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(summary.execution_id.to_string());
    let execution_path = state_dir.join("execution.json");
    let checkpoint_path = state_dir.join("checkpoint.json");

    let mut execution_value = read_json(&execution_path);
    execution_value["status"] = json!("Running");
    execution_value["completed_at"] = json!(null);
    write_json(&execution_path, &execution_value);

    let mut checkpoint_value = read_json(&checkpoint_path);
    if let Some(map) = checkpoint_value.as_object_mut() {
        map.insert("ready_queue".to_string(), json!([]));
        map.insert(
            "task_iterations".to_string(),
            json!({"task1": 1, "task2": 1}),
        );
        map.insert("total_iterations".to_string(), json!(2));
        if let Some(completed) = map.get_mut("completed").and_then(Value::as_object_mut) {
            completed.retain(|key, _| key == "task1");
        }
    }
    write_json(&checkpoint_path, &checkpoint_value);

    let resume_registry = build_registry(workspace.path().to_path_buf(), settings);
    let err = executor::resume_workflow(
        resume_registry,
        workspace.path().to_path_buf(),
        summary.execution_id,
        true, // allow_workflow_change=true — guard still fires
    )
    .await
    .expect_err("resume of inconsistent checkpoint must fail even with allow_workflow_change");

    assert_eq!(err.code, "WFG-RESUME-002");
}
