use newton_core::workflow::{
    expression::ExpressionEngine,
    io::{
        evaluate_result_map, validate_error_schema, validate_input_schema, validate_output_schema,
        CompletionStatus,
    },
    operator::StateView,
    schema::{IoBlock, WorkflowDocument},
};
use serde_json::{json, Value};
use std::fs;
use tempfile::NamedTempFile;

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
