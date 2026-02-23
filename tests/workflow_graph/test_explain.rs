use insta::assert_snapshot;
use newton::core::workflow_graph::{explain, schema};
use serde_json::json;
use std::fs;
use tempfile::NamedTempFile;

#[test]
fn explain_evaluates_context_and_marks_runtime_expressions() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context:
    env: "dev"
  settings:
    entry_task: start
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 3
    max_workflow_iterations: 10
  tasks:
    - id: start
      operator: SetContextOperator
      params:
        computed:
          $expr: "context.build_number + 1"
        runtime_only:
          $expr: "tasks.start.status == 'success'"
      transitions:
        - to: done
          priority: 10
          when:
            $expr: "context.env == 'prod'"
        - to: done
          priority: 1
    - id: done
      operator: NoOpOperator
      params: {}
"#;

    let file = NamedTempFile::new().expect("temp file");
    fs::write(file.path(), workflow).expect("write workflow");
    let document = schema::parse_workflow(file.path()).expect("parse workflow");

    let overrides = vec![
        ("build_number".to_string(), json!(7)),
        ("env".to_string(), json!("prod")),
    ];
    let triggers = json!({});
    let outcome = explain::build_explain_outcome(&document, &overrides, &triggers)
        .expect("build explain output");
    assert!(!outcome.has_blocking_diagnostics());

    assert_snapshot!(
        serde_json::to_string_pretty(&outcome.output).expect("serialize explain output"),
        @r###"
    {
      "settings": {
        "artifact_storage": {
          "base_path": ".newton/artifacts",
          "cleanup_policy": "lru",
          "max_artifact_bytes": 104857600,
          "max_inline_bytes": 65536,
          "max_total_bytes": 1073741824,
          "retention_hours": 168
        },
        "checkpoint": {
          "checkpoint_enabled": true,
          "checkpoint_interval_seconds": 30,
          "checkpoint_keep_history": false,
          "checkpoint_on_task_complete": true
        },
        "command_operator": {
          "allow_shell": false
        },
        "completion": {
          "goal_gate_failure_behavior": "fail",
          "require_goal_gates": true,
          "stop_on_terminal": true,
          "success_requires_no_task_failures": true
        },
        "continue_on_error": false,
        "entry_task": "start",
        "human": {
          "audit_path": ".newton/state/workflows",
          "default_timeout_seconds": 86400
        },
        "max_task_iterations": 3,
        "max_time_seconds": 60,
        "max_workflow_iterations": 10,
        "parallel_limit": 1,
        "redaction": {
          "redact_keys": [
            "token",
            "password",
            "secret"
          ]
        },
        "required_triggers": [],
        "webhook": {
          "auth_token_env": "NEWTON_WEBHOOK_TOKEN",
          "bind": "127.0.0.1:8787",
          "enabled": false,
          "max_body_bytes": 1048576
        }
      },
      "context": {
        "build_number": 7,
        "env": "prod"
      },
      "triggers": {},
      "tasks": [
        {
          "id": "start",
          "operator": "SetContextOperator",
          "params": {
            "computed": 8,
            "runtime_only": "(runtime)"
          },
          "transitions": [
            {
              "target": "done",
              "priority": 1,
              "when": "(always)"
            },
            {
              "target": "done",
              "priority": 10,
              "when": "context.env == 'prod'"
            }
          ]
        },
        {
          "id": "done",
          "operator": "NoOpOperator",
          "params": {},
          "transitions": []
        }
      ]
    }
    "###);
}

#[test]
fn explain_reports_blocking_expression_errors() {
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
  tasks:
    - id: start
      operator: SetContextOperator
      params:
        invalid:
          $expr: "context.foo +"
"#;

    let file = NamedTempFile::new().expect("temp file");
    fs::write(file.path(), workflow).expect("write workflow");
    let document = schema::parse_workflow(file.path()).expect("parse workflow");

    let triggers = json!({});
    let outcome =
        explain::build_explain_outcome(&document, &[], &triggers).expect("build explain outcome");
    assert!(outcome.has_blocking_diagnostics());
    assert_eq!(outcome.diagnostics.len(), 1);
}
