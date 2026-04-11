use insta::assert_snapshot;
use newton::workflow::{explain, schema};
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
        @r#"
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
        "stream_agent_stdout": false,
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
    "#);
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

fn create_sample_workflow_for_prose_test() -> &'static str {
    r#"
version: "2.0"
mode: workflow_graph
workflow:
  context:
    env: "dev"
    build_number: 42
  settings:
    entry_task: start
    max_time_seconds: 300
    parallel_limit: 2
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: start
      operator: SetContextOperator
      params:
        message: "Starting build"
        computed:
          $expr: "context.build_number + 1"
        runtime_only:
          $expr: "tasks.start.status == 'success'"
      transitions:
        - to: deploy
          priority: 10
          when:
            $expr: "context.env == 'prod'"
        - to: test
          priority: 1
    - id: test
      operator: CommandOperator
      params:
        command: "npm test"
        working_directory: "/app"
      transitions:
        - to: deploy
          priority: 1
    - id: deploy
      operator: NoOpOperator
      params:
        target: "production"
"#
}

fn create_explain_outcome_from_workflow(workflow: &str) -> explain::ExplainOutcome {
    let file = NamedTempFile::new().expect("temp file");
    fs::write(file.path(), workflow).expect("write workflow");
    let document = schema::parse_workflow(file.path()).expect("parse workflow");

    let overrides = vec![("env".to_string(), json!("dev"))];
    let triggers = json!({"workflow_trigger": "manual"});
    explain::build_explain_outcome(&document, &overrides, &triggers).expect("build explain output")
}

fn verify_prose_structural_elements(prose: &str) {
    assert!(prose.contains("# Workflow Execution Instructions"));
    assert!(prose.contains("## Context"));
    assert!(prose.contains("## Trigger Information"));
    assert!(prose.contains("## Workflow Settings"));
    assert!(prose.contains("## Execution Steps"));
}

fn verify_prose_task_content(prose: &str) {
    // Verify task IDs are present
    assert!(prose.contains("start"));
    assert!(prose.contains("test"));
    assert!(prose.contains("deploy"));

    // Verify operator names are present
    assert!(prose.contains("SetContextOperator"));
    assert!(prose.contains("CommandOperator"));
    assert!(prose.contains("NoOpOperator"));
}

fn verify_prose_transition_content(prose: &str) {
    // Verify transition information is present
    assert!(prose.contains("priority 1"));
    assert!(prose.contains("priority 10"));
}

fn verify_prose_runtime_handling(prose: &str) {
    // Verify runtime placeholder handling
    assert!(prose.contains("(runtime)"));
    assert!(prose.contains("value provided at runtime"));
}

fn verify_prose_execution_notes(prose: &str) {
    // Verify execution notes are included
    assert!(prose.contains("## Execution Notes"));
    assert!(prose.contains("Transition conditions should be evaluated"));
}

#[test]
fn explain_prose_format_contains_required_elements() {
    let workflow = create_sample_workflow_for_prose_test();
    let outcome = create_explain_outcome_from_workflow(workflow);
    let prose = explain::format_explain_prose(&outcome.output).expect("format prose");

    verify_prose_structural_elements(&prose);
    verify_prose_task_content(&prose);
    verify_prose_transition_content(&prose);
    verify_prose_runtime_handling(&prose);
    verify_prose_execution_notes(&prose);
}

fn create_snapshot_test_workflow() -> &'static str {
    r#"
version: "2.0"
mode: workflow_graph
workflow:
  context:
    app_name: "test-app"
    version: "1.0.0"
  settings:
    entry_task: build
    max_time_seconds: 120
    parallel_limit: 1
    max_task_iterations: 3
    max_workflow_iterations: 10
  tasks:
    - id: build
      operator: CommandOperator
      params:
        command: "npm run build"
        working_directory:
          $expr: "context.app_name"
        timeout_seconds: 60
      transitions:
        - to: test
          priority: 1
          when:
            $expr: "context.version != ''"
    - id: test
      operator: CommandOperator
      params:
        command: "npm test"
        env:
          NODE_ENV: "test"
          APP_VERSION:
            $expr: "context.version"
      transitions:
        - to: done
          priority: 1
    - id: done
      operator: NoOpOperator
      params: {}
"#
}

fn create_explain_outcome_for_snapshot_test() -> explain::ExplainOutcome {
    let workflow = create_snapshot_test_workflow();
    let file = NamedTempFile::new().expect("temp file");
    fs::write(file.path(), workflow).expect("write workflow");
    let document = schema::parse_workflow(file.path()).expect("parse workflow");

    let overrides = vec![];
    let triggers = json!({"input_file": "/path/to/input.txt"});
    explain::build_explain_outcome(&document, &overrides, &triggers).expect("build explain output")
}

#[test]
fn explain_prose_format_snapshot_test() {
    let outcome = create_explain_outcome_for_snapshot_test();
    let prose = explain::format_explain_prose(&outcome.output).expect("format prose");

    assert_snapshot!(prose, @r#"
    # Workflow Execution Instructions

    This document contains complete instructions for executing a workflow. All steps, conditions, and parameters are included to enable execution without access to the original workflow file or Newton runtime.

    ## Context

    Initial workflow context:
    ```json
    {
      "app_name": "test-app",
      "version": "1.0.0"
    }
    ```

    ## Trigger Information

    Workflow triggers and payload:
    ```json
    {
      "input_file": "/path/to/input.txt"
    }
    ```

    ## Workflow Settings

    Effective workflow settings:
    ```json
    {
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
      "entry_task": "build",
      "human": {
        "audit_path": ".newton/state/workflows",
        "default_timeout_seconds": 86400
      },
      "max_task_iterations": 3,
      "max_time_seconds": 120,
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
      "stream_agent_stdout": false,
      "webhook": {
        "auth_token_env": "NEWTON_WEBHOOK_TOKEN",
        "bind": "127.0.0.1:8787",
        "enabled": false,
        "max_body_bytes": 1048576
      }
    }
    ```

    ## Execution Steps

    Execute the following tasks according to their transition conditions. Tasks are listed in dependency order, but actual execution depends on the transition logic.

    ### 1: build (CommandOperator)

    **Parameters:**
    ```json
    {
      "command": "npm run build",
      "timeout_seconds": 60,
      "working_directory": "test-app"
    }
    ```

    **Transitions after completion:**
    - Go to task 'test' with priority 1 when: context.version != ''

    ### 2: test (CommandOperator)

    **Parameters:**
    ```json
    {
      "command": "npm test",
      "env": {
        "APP_VERSION": "1.0.0",
        "NODE_ENV": "test"
      }
    }
    ```

    **Transitions after completion:**
    - Go to task 'done' with priority 1 when: (always)

    ### 3: done (NoOpOperator)

    **Parameters:**
    ```json
    {}
    ```

    **Transitions:** None (terminal task)

    ## Execution Notes

    - Parameters marked as "(runtime)" will be provided or calculated during execution
    - Transition conditions should be evaluated after each task completes
    - Execute transitions in priority order (lower numbers have higher priority)
    - If no transition conditions match, the workflow terminates
    - Tasks without transitions are terminal tasks that end the workflow when completed
    "#);
}
