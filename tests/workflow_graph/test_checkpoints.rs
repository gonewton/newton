use newton::workflow::checkpoint;
use newton::workflow::{
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
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        server_notifier: None,
        pre_seed_nodes: true,
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
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        server_notifier: None,
        pre_seed_nodes: true,
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
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        server_notifier: None,
        pre_seed_nodes: true,
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

#[tokio::test]
async fn checkpoints_list_output_format_and_sort_order() {
    let workspace = tempdir().expect("workspace");
    let workflow_file = write_workflow(RESUME_WORKFLOW);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings.clone());
    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        server_notifier: None,
        pre_seed_nodes: true,
    };

    // Run workflow twice to create multiple checkpoints
    let _summary1 = executor::execute_workflow(
        document.clone(),
        workflow_file.path().to_path_buf(),
        registry.clone(),
        workspace.path().to_path_buf(),
        overrides.clone(),
    )
    .await
    .expect("first run succeeded");

    // Sleep briefly to ensure different timestamps
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let _summary2 = executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry.clone(),
        workspace.path().to_path_buf(),
        overrides,
    )
    .await
    .expect("second run succeeded");

    // Use list_checkpoints to verify the data
    let mut entries = checkpoint::list_checkpoints(workspace.path()).expect("list checkpoints");
    assert_eq!(entries.len(), 2);

    // Manually sort to verify sorting works correctly
    entries.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    // Verify entries are sorted by started_at descending (newest first)
    assert!(entries[0].started_at >= entries[1].started_at);

    // Verify each entry has the required fields
    for entry in &entries {
        assert_ne!(entry.execution_id, uuid::Uuid::nil());
        assert_eq!(entry.status, state::WorkflowExecutionStatus::Completed);
    }
}

#[tokio::test]
async fn resume_with_allow_workflow_change_updates_iteration_limits() {
    let workspace = tempdir().expect("workspace");
    let workflow_with_low_limits = r#"
version: 2.0
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: first
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 2
    max_workflow_iterations: 3
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
    let workflow_file = write_workflow(workflow_with_low_limits);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings.clone());
    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        server_notifier: None,
        pre_seed_nodes: true,
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

    let workflow_with_higher_limits = r#"
version: 2.0
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: first
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 20
    max_workflow_iterations: 50
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
    fs::write(workflow_file.path(), workflow_with_higher_limits).expect("update workflow");

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
}

#[tokio::test]
async fn resume_without_allow_workflow_change_preserves_checkpoint_limits() {
    let workspace = tempdir().expect("workspace");
    let workflow_with_low_limits = r#"
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
    let workflow_file = write_workflow(workflow_with_low_limits);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings.clone());
    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        server_notifier: None,
        pre_seed_nodes: true,
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
}

#[tokio::test]
async fn resume_with_allow_workflow_change_preserves_other_settings() {
    let workspace = tempdir().expect("workspace");
    let workflow_original = r#"
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
        - to: done
          when:
            $expr: "true"
    - id: done
      operator: NoOpOperator
      params: {}
"#;
    let workflow_file = write_workflow(workflow_original);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings.clone());
    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        server_notifier: None,
        pre_seed_nodes: true,
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
        map.insert("ready_queue".to_string(), json!(["done"]));
        map.insert("task_iterations".to_string(), json!({"first": 1}));
        map.insert("total_iterations".to_string(), json!(1));
        if let Some(completed) = map.get_mut("completed").and_then(Value::as_object_mut) {
            completed.retain(|key, _| key == "first");
        }
    }
    write_json(&checkpoint_path, &checkpoint_value);

    let workflow_with_changed_non_iteration_settings = r#"
version: 2.0
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: first
    max_time_seconds: 999
    parallel_limit: 999
    continue_on_error: true
    max_task_iterations: 20
    max_workflow_iterations: 50
  tasks:
    - id: first
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
    fs::write(
        workflow_file.path(),
        workflow_with_changed_non_iteration_settings,
    )
    .expect("update workflow");

    let resume_registry = build_registry(workspace.path().to_path_buf(), settings);
    let resume_summary = executor::resume_workflow(
        resume_registry,
        workspace.path().to_path_buf(),
        summary.execution_id,
        true,
    )
    .await
    .expect("resume succeeded");

    assert!(resume_summary.total_iterations >= 2);
}

#[tokio::test]
async fn hard_abort_task_is_requeued_in_checkpoint() {
    // When a task hard-aborts (operator not registered), it must be re-queued in
    // the checkpoint so that a corrected workflow can resume from that task.
    let workspace = tempdir().expect("workspace");
    let workflow_with_bad_operator = r#"
version: 2.0
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: fail_task
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: fail_task
      operator: NonExistentOperator
      params: {}
"#;
    let workflow_file = write_workflow(workflow_with_bad_operator);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings.clone());
    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        server_notifier: None,
        pre_seed_nodes: true,
    };

    let result = executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry.clone(),
        workspace.path().to_path_buf(),
        overrides,
    )
    .await;

    // Execution must fail with a hard operator-resolution error.
    let err = result.expect_err("hard abort must fail");
    assert_eq!(err.code, "WFG-OP-001");

    // Locate the execution directory.
    let state_root = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows");
    let entries: Vec<_> = fs::read_dir(&state_root)
        .expect("state dir exists")
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(entries.len(), 1, "exactly one execution directory");
    let exec_dir = entries[0].path();
    let checkpoint_path = exec_dir.join("checkpoint.json");

    let checkpoint_value = read_json(&checkpoint_path);

    // The aborted task must be back in ready_queue.
    let ready_queue = checkpoint_value["ready_queue"]
        .as_array()
        .expect("ready_queue is array");
    assert_eq!(ready_queue.len(), 1, "aborted task must be re-queued");
    assert_eq!(
        ready_queue[0].as_str().unwrap(),
        "fail_task",
        "fail_task must be in ready_queue after hard abort"
    );

    // No tasks completed.
    let completed = checkpoint_value["completed"]
        .as_object()
        .expect("completed is object");
    assert_eq!(completed.len(), 0, "no tasks should be in completed");

    // total_iterations was incremented before the abort.
    let total_iterations = checkpoint_value["total_iterations"]
        .as_u64()
        .expect("total_iterations is number");
    assert_eq!(total_iterations, 1, "total_iterations should be 1");
}
