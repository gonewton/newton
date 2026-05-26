use newton_core::workflow::{
    executor::{self, ExecutionOverrides},
    expression::ExpressionEngine,
    io::{
        evaluate_result_map, validate_error_schema, validate_input_schema, validate_input_size,
        validate_output_schema, validate_output_size, CompletionStatus,
    },
    operator::{OperatorRegistry, StateView},
    operators, schema,
    schema::{IoBlock, WorkflowDocument},
};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::Path;
use tempfile::{tempdir, NamedTempFile};

fn write_workflow(yaml: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("temp file");
    write!(file, "{yaml}").expect("write workflow");
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

fn default_overrides() -> ExecutionOverrides {
    ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        sink: None,
        pre_seed_nodes: true,
    }
}

fn build_registry(
    workspace: std::path::PathBuf,
    settings: newton_core::workflow::state::GraphSettings,
) -> OperatorRegistry {
    let mut builder = OperatorRegistry::builder();
    operators::register_builtins(&mut builder, workspace, settings);
    builder.build()
}

fn make_state_view(context: Value, tasks: Value, triggers: Value) -> StateView {
    StateView::new(context, tasks, triggers)
}

fn make_io_block_with_result_map(entries: &[(&str, &str)]) -> IoBlock {
    use indexmap::IndexMap;
    let mut map = IndexMap::new();
    for (k, v) in entries {
        map.insert(k.to_string(), json!(v));
    }
    IoBlock {
        result_map: Some(map),
        ..Default::default()
    }
}

// ─── evaluate_result_map ─────────────────────────────────────────────────────

#[test]
fn evaluate_result_map_returns_none_when_absent() {
    let io = IoBlock::default();
    let state = make_state_view(json!({}), json!({}), json!({}));
    let engine = ExpressionEngine::default();
    let result = evaluate_result_map(&io, &state, &engine).expect("no error");
    assert!(result.is_none());
}

#[test]
fn evaluate_result_map_literal_values() {
    let io = make_io_block_with_result_map(&[("status", "ok"), ("count", "5")]);
    let state = make_state_view(json!({}), json!({}), json!({}));
    let engine = ExpressionEngine::default();
    let result = evaluate_result_map(&io, &state, &engine)
        .expect("no error")
        .expect("result is Some");
    assert_eq!(result["status"], json!("ok"));
    assert_eq!(result["count"], json!("5"));
}

#[test]
fn evaluate_result_map_expr_values() {
    use indexmap::IndexMap;
    let mut map = IndexMap::new();
    map.insert("sum".to_string(), json!("$expr: 1 + 2"));
    let io = IoBlock {
        result_map: Some(map),
        ..Default::default()
    };
    let state = make_state_view(json!({}), json!({}), json!({}));
    let engine = ExpressionEngine::default();
    let result = evaluate_result_map(&io, &state, &engine)
        .expect("no error")
        .expect("result is Some");
    assert_eq!(result["sum"], json!(3));
}

#[test]
fn evaluate_result_map_expr_error_returns_wfg_io_005() {
    use indexmap::IndexMap;
    let mut map = IndexMap::new();
    map.insert("bad".to_string(), json!("$expr: invalid +++"));
    let io = IoBlock {
        result_map: Some(map),
        ..Default::default()
    };
    let state = make_state_view(json!({}), json!({}), json!({}));
    let engine = ExpressionEngine::default();
    let err = evaluate_result_map(&io, &state, &engine).expect_err("expected error");
    assert_eq!(err.code, "WFG-IO-005", "expected WFG-IO-005, got {:?}", err);
}

// ─── validate_input_schema ───────────────────────────────────────────────────

#[test]
fn validate_input_schema_passes_for_conformant_payload() {
    let schema = json!({
        "type": "object",
        "properties": { "repo": { "type": "string" } },
        "required": ["repo"]
    });
    let payload = json!({ "repo": "my-repo" });
    validate_input_schema(&schema, &payload).expect("should pass");
}

#[test]
fn validate_input_schema_fails_for_missing_required_field() {
    let schema = json!({
        "type": "object",
        "properties": { "repo": { "type": "string" } },
        "required": ["repo"]
    });
    let payload = json!({});
    let err = validate_input_schema(&schema, &payload).expect_err("should fail");
    assert_eq!(err.code, "WFG-IO-002");
    assert!(
        err.message.contains("input_schema"),
        "message should mention input_schema: {}",
        err.message
    );
}

// ─── validate_output_schema ──────────────────────────────────────────────────

#[test]
fn validate_output_schema_passes_for_conformant_result() {
    let schema = json!({
        "type": "object",
        "properties": { "status": { "type": "string", "enum": ["ok", "failed"] } },
        "required": ["status"]
    });
    let result = json!({ "status": "ok" });
    validate_output_schema(&schema, &result).expect("should pass");
}

#[test]
fn validate_output_schema_fails_for_missing_required_field() {
    let schema = json!({
        "type": "object",
        "properties": { "status": { "type": "string" } },
        "required": ["status"]
    });
    let result = json!({});
    let err = validate_output_schema(&schema, &result).expect_err("should fail");
    assert_eq!(err.code, "WFG-IO-003");
    assert!(
        err.message.contains("output_schema"),
        "message should mention output_schema: {}",
        err.message
    );
}

// ─── validate_input_size (WFG-IO-001) ────────────────────────────────────────

#[test]
fn validate_input_size_passes_when_under_limit() {
    let payload = json!({ "repo": "my-repo" });
    validate_input_size(&payload, 65536).expect("should pass when payload is under limit");
}

#[test]
fn validate_input_size_fails_with_wfg_io_001_when_over_limit() {
    // max_input_bytes: 1 — any non-trivial payload will exceed this
    let payload = json!({ "repo": "my-repo" });
    let err = validate_input_size(&payload, 1).expect_err("should fail when payload exceeds limit");
    assert_eq!(err.code, "WFG-IO-001", "expected WFG-IO-001, got {:?}", err);
    assert!(
        err.message.contains("max_input_bytes"),
        "message should mention max_input_bytes: {}",
        err.message
    );
}

// ─── validate_output_size (WFG-IO-003 via max_output_bytes) ──────────────────

#[test]
fn validate_output_size_passes_when_under_limit() {
    let result = json!({ "status": "ok" });
    validate_output_size(&result, 65536).expect("should pass when result is under limit");
}

#[test]
fn validate_output_size_fails_with_wfg_io_003_when_over_limit() {
    // max_output_bytes: 1 — any non-trivial result will exceed this
    let result = json!({ "status": "ok" });
    let err = validate_output_size(&result, 1).expect_err("should fail when result exceeds limit");
    assert_eq!(err.code, "WFG-IO-003", "expected WFG-IO-003, got {:?}", err);
    assert!(
        err.message.contains("max_output_bytes"),
        "message should mention max_output_bytes: {}",
        err.message
    );
}

// ─── validate_error_schema ───────────────────────────────────────────────────

#[test]
fn validate_error_schema_passes_for_conformant_payload() {
    let schema = json!({
        "type": "object",
        "properties": { "reason": { "type": "string" } },
        "required": ["reason"]
    });
    let payload = json!({ "reason": "tests failed" });
    validate_error_schema(&schema, &payload).expect("should pass");
}

#[test]
fn validate_error_schema_fails_for_invalid_payload() {
    let schema = json!({
        "type": "object",
        "properties": { "reason": { "type": "string" } },
        "required": ["reason"]
    });
    let payload = json!({});
    let err = validate_error_schema(&schema, &payload).expect_err("should fail");
    assert_eq!(err.code, "WFG-IO-004");
    assert!(
        err.message.contains("error_schema"),
        "message should mention error_schema: {}",
        err.message
    );
}

// ─── io block in workflow YAML ────────────────────────────────────────────────

#[test]
fn parse_workflow_with_io_block() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: start
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 3
    max_workflow_iterations: 10
    io:
      input_schema:
        type: object
        properties:
          repo:
            type: string
        required:
          - repo
      result_map:
        status: ok
      output_schema:
        type: object
        properties:
          status:
            type: string
        required:
          - status
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      terminal: success
"#;
    let file = NamedTempFile::new().expect("temp file");
    fs::write(file.path(), workflow).expect("write workflow");
    let doc: WorkflowDocument =
        newton_core::workflow::schema::parse_workflow(file.path()).expect("parse workflow");
    let io = &doc.workflow.settings.io;
    assert!(io.input_schema.is_some(), "input_schema should be present");
    assert!(
        io.output_schema.is_some(),
        "output_schema should be present"
    );
    assert!(io.result_map.is_some(), "result_map should be present");
}

// ─── CompletionEnvelope shapes ────────────────────────────────────────────────

#[test]
fn completion_envelope_success_shape() {
    use newton_core::workflow::io::CompletionEnvelope;
    use uuid::Uuid;
    let id = Uuid::new_v4();
    let env = CompletionEnvelope::success(id, Some(json!({ "status": "ok" })));
    assert_eq!(env.status, CompletionStatus::Success);
    assert_eq!(env.execution_id, Some(id));
    assert!(env.error.is_none());
    assert_eq!(env.result, Some(json!({ "status": "ok" })));
    assert_eq!(env.schema_version, "1");
}

#[test]
fn completion_envelope_internal_error_shape() {
    use newton_core::workflow::io::{CompletionEnvelope, CompletionError};
    let env = CompletionEnvelope::internal_error(CompletionError {
        code: Some("WFG-IO-002".to_string()),
        category: "ValidationError".to_string(),
        message: "trigger payload invalid".to_string(),
        error_payload: None,
    });
    assert_eq!(env.status, CompletionStatus::InternalError);
    assert!(env.execution_id.is_none());
    assert!(env.result.is_none());
    let err = env.error.expect("error field present");
    assert_eq!(err.code, Some("WFG-IO-002".to_string()));
}

// ─── WorkflowOperator child result surface (AC 17, AC 18) ────────────────────

/// AC 17: parent workflow accessing tasks['run-child'].result.status receives
/// the child's result_map output.
#[tokio::test]
async fn ac17_workflow_operator_child_result_accessible_in_parent() {
    let workspace = tempdir().expect("workspace");

    let child_yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: child-task
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 3
    max_workflow_iterations: 10
    io:
      result_map:
        status: done
  tasks:
    - id: child-task
      operator: NoOpOperator
      params: {}
      terminal: success
"#;
    fs::write(workspace.path().join("child_workflow.yaml"), child_yaml)
        .expect("write child workflow");

    let parent_yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: run-child
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 3
    max_workflow_iterations: 10
    io:
      result_map:
        child_status: "$expr: tasks[\"run-child\"].result.status"
  tasks:
    - id: run-child
      operator: WorkflowOperator
      params:
        workflow_path: child_workflow.yaml
      terminal: success
"#;
    let parent_path = workspace.path().join("parent.yaml");
    fs::write(&parent_path, parent_yaml).expect("write parent workflow");

    let document = schema::load_workflow(&parent_path).expect("parse parent workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings);

    let summary = executor::execute_workflow(
        document,
        parent_path,
        registry,
        workspace.path().to_path_buf(),
        default_overrides(),
    )
    .await
    .expect("parent workflow should succeed");

    let result = summary
        .result
        .expect("parent should have result from result_map");
    assert_eq!(
        result["child_status"],
        json!("done"),
        "parent should read child result_map output via tasks['run-child'].result.status"
    );
}

/// AC 18: child with no result_map → parent sees tasks['child'].result as null.
#[tokio::test]
async fn ac18_workflow_operator_no_result_map_gives_null_result() {
    let workspace = tempdir().expect("workspace");

    let child_yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: child-task
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 3
    max_workflow_iterations: 10
  tasks:
    - id: child-task
      operator: NoOpOperator
      params: {}
      terminal: success
"#;
    fs::write(workspace.path().join("child_workflow.yaml"), child_yaml)
        .expect("write child workflow");

    let parent_yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: run-child
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 3
    max_workflow_iterations: 10
    io:
      result_map:
        child_ran: "true"
  tasks:
    - id: run-child
      operator: WorkflowOperator
      params:
        workflow_path: child_workflow.yaml
      terminal: success
"#;
    let parent_path = workspace.path().join("parent.yaml");
    fs::write(&parent_path, parent_yaml).expect("write parent workflow");

    let document = schema::load_workflow(&parent_path).expect("parse parent workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings);

    let summary = executor::execute_workflow(
        document,
        parent_path,
        registry,
        workspace.path().to_path_buf(),
        default_overrides(),
    )
    .await
    .expect("parent workflow should succeed");

    let child_task_record = summary
        .completed_tasks
        .get("run-child")
        .expect("run-child task should be in completed_tasks");
    assert!(
        child_task_record.output["result"].is_null(),
        "child with no result_map should produce null result in task output; got {:?}",
        child_task_record.output["result"]
    );

    let result = summary.result.expect("parent should have its own result");
    assert_eq!(result["child_ran"], json!("true"));
}

// ─── Resume io_snapshot guard (AC 24–27) ─────────────────────────────────────

const IO_SNAPSHOT_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: first
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
    io:
      result_map:
        status: ok
  tasks:
    - id: first
      operator: NoOpOperator
      params: {}
      transitions:
        - to: second
    - id: second
      operator: NoOpOperator
      params: {}
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      params: {}
"#;

/// Runs IO_SNAPSHOT_WORKFLOW to completion, then patches checkpoint.json and
/// execution.json to simulate a mid-run partial state (only "first" completed,
/// "second" queued). Returns the workspace TempDir and execution UUID so the
/// caller can perform additional checkpoint manipulation before resuming.
async fn run_and_make_partial_io_checkpoint(
    workflow_file: &NamedTempFile,
    workspace: &tempfile::TempDir,
) -> uuid::Uuid {
    let document = schema::load_workflow(workflow_file.path()).expect("parse workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings);

    let summary = executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry,
        workspace.path().to_path_buf(),
        default_overrides(),
    )
    .await
    .expect("first run succeeded");

    let state_dir = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(summary.execution_id.to_string());

    let mut exec_val = read_json(&state_dir.join("execution.json"));
    exec_val["status"] = Value::String("Running".to_string());
    exec_val["completed_at"] = Value::Null;
    if let Some(arr) = exec_val["task_runs"].as_array_mut() {
        arr.retain(|e| e["task_id"] == "first");
    }
    write_json(&state_dir.join("execution.json"), &exec_val);

    let mut ckpt_val = read_json(&state_dir.join("checkpoint.json"));
    if let Some(map) = ckpt_val.as_object_mut() {
        map.insert("ready_queue".to_string(), json!(["second"]));
        map.insert("task_iterations".to_string(), json!({"first": 1}));
        map.insert("total_iterations".to_string(), json!(1));
        if let Some(completed) = map.get_mut("completed").and_then(Value::as_object_mut) {
            completed.retain(|k, _| k == "first");
        }
    }
    write_json(&state_dir.join("checkpoint.json"), &ckpt_val);

    summary.execution_id
}

/// AC 24: resuming a run whose io_snapshot matches the current workflow's io
/// block succeeds without error.
#[tokio::test]
async fn ac24_resume_matching_io_snapshot_succeeds() {
    let workspace = tempdir().expect("workspace");
    let workflow_file = write_workflow(IO_SNAPSHOT_WORKFLOW);
    let execution_id = run_and_make_partial_io_checkpoint(&workflow_file, &workspace).await;

    let document = schema::load_workflow(workflow_file.path()).expect("parse");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings);

    let result = executor::resume_workflow(
        registry,
        workspace.path().to_path_buf(),
        execution_id,
        false,
    )
    .await;
    assert!(
        result.is_ok(),
        "resume with matching io_snapshot should succeed; err={:?}",
        result.err()
    );
}

/// AC 25: resuming a run whose io_snapshot differs from the current io block
/// fails with WFG-CKPT-001 unless --allow-workflow-change is passed.
#[tokio::test]
async fn ac25_resume_mismatched_io_snapshot_fails_with_ckpt_001() {
    let workspace = tempdir().expect("workspace");
    let workflow_file = write_workflow(IO_SNAPSHOT_WORKFLOW);
    let execution_id = run_and_make_partial_io_checkpoint(&workflow_file, &workspace).await;

    // Patch checkpoint.json: set io_snapshot to an empty object (different from current io)
    let state_dir = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(execution_id.to_string());
    let mut ckpt_val = read_json(&state_dir.join("checkpoint.json"));
    ckpt_val["io_snapshot"] = json!({});
    write_json(&state_dir.join("checkpoint.json"), &ckpt_val);

    let document = schema::load_workflow(workflow_file.path()).expect("parse");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings);

    let err = executor::resume_workflow(
        registry,
        workspace.path().to_path_buf(),
        execution_id,
        false,
    )
    .await
    .expect_err("mismatched io_snapshot should block resume");
    assert_eq!(
        err.code, "WFG-CKPT-001",
        "expected WFG-CKPT-001 on io_snapshot mismatch; got {:?}",
        err
    );
}

/// AC 26: resuming with --allow-workflow-change re-validates the original
/// trigger payload against the new input_schema; if validation fails, resume
/// is blocked with WFG-IO-002.
#[tokio::test]
async fn ac26_resume_allow_workflow_change_revalidates_trigger_payload() {
    let workspace = tempdir().expect("workspace");

    // Initial workflow: no input_schema, but has io block (result_map) so io_snapshot is stored.
    let initial_yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: first
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
    io:
      result_map:
        status: ok
  tasks:
    - id: first
      operator: NoOpOperator
      params: {}
      transitions:
        - to: second
    - id: second
      operator: NoOpOperator
      params: {}
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      params: {}
"#;
    let workflow_file = write_workflow(initial_yaml);
    let execution_id = run_and_make_partial_io_checkpoint(&workflow_file, &workspace).await;

    // Rewrite the workflow file to add a strict input_schema (requires "branch" field).
    // The original trigger payload is {} (no branch), so re-validation will fail.
    let updated_yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: first
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
    io:
      input_schema:
        type: object
        properties:
          branch:
            type: string
        required:
          - branch
      result_map:
        status: ok
  tasks:
    - id: first
      operator: NoOpOperator
      params: {}
      transitions:
        - to: second
    - id: second
      operator: NoOpOperator
      params: {}
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      params: {}
"#;
    fs::write(workflow_file.path(), updated_yaml).expect("rewrite workflow");

    // Patch checkpoint: set io_snapshot to something different so the re-validate branch runs.
    let state_dir = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(execution_id.to_string());
    let mut ckpt_val = read_json(&state_dir.join("checkpoint.json"));
    ckpt_val["io_snapshot"] = json!({"result_map": {"status": "ok"}});
    write_json(&state_dir.join("checkpoint.json"), &ckpt_val);

    let document = schema::load_workflow(workflow_file.path()).expect("parse updated workflow");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings);

    // allow_workflow_change=true: hash check skipped, but payload re-validated against new schema.
    let err =
        executor::resume_workflow(registry, workspace.path().to_path_buf(), execution_id, true)
            .await
            .expect_err("re-validation of original payload against new schema should fail");
    assert_eq!(
        err.code, "WFG-IO-002",
        "expected WFG-IO-002 when original trigger payload fails new input_schema; got {:?}",
        err
    );
}

/// AC 27: resuming a checkpoint written before this spec (no io_snapshot field)
/// succeeds (backward-compatible).
#[tokio::test]
async fn ac27_resume_old_checkpoint_without_io_snapshot_succeeds() {
    let workspace = tempdir().expect("workspace");
    let workflow_file = write_workflow(IO_SNAPSHOT_WORKFLOW);
    let execution_id = run_and_make_partial_io_checkpoint(&workflow_file, &workspace).await;

    // Remove io_snapshot from checkpoint.json to simulate an old-format checkpoint.
    let state_dir = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(execution_id.to_string());
    let mut ckpt_val = read_json(&state_dir.join("checkpoint.json"));
    if let Some(map) = ckpt_val.as_object_mut() {
        map.remove("io_snapshot");
    }
    write_json(&state_dir.join("checkpoint.json"), &ckpt_val);

    let document = schema::load_workflow(workflow_file.path()).expect("parse");
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.path().to_path_buf(), settings);

    let result = executor::resume_workflow(
        registry,
        workspace.path().to_path_buf(),
        execution_id,
        false,
    )
    .await;
    assert!(
        result.is_ok(),
        "resume of old checkpoint without io_snapshot should succeed (backward-compat); err={:?}",
        result.err()
    );
}
