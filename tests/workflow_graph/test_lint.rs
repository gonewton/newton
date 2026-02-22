use insta::assert_snapshot;
use newton::core::workflow_graph::{
    lint::{LintRegistry, LintSeverity},
    schema,
};
use std::fs;
use tempfile::NamedTempFile;

#[test]
fn lint_results_are_stably_sorted() {
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
      operator: CommandOperator
      params:
        cmd: "echo hello"
        shell: true
      transitions:
        - to: missing
          priority: 10
          when:
            $expr: "1 +"
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: done
          priority: 1
          when:
            $expr: "1 + 1"
    - id: done
      operator: AssertCompletedOperator
      params:
        require: ["ghost"]
"#;

    let file = NamedTempFile::new().expect("temp file");
    fs::write(file.path(), workflow).expect("write workflow");
    let document = schema::parse_workflow(file.path()).expect("parse workflow");

    let results = LintRegistry::new().run(&document);
    assert!(!results.is_empty());
    for pair in results.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        let left_rank = severity_rank(left.severity);
        let right_rank = severity_rank(right.severity);
        assert!(
            left_rank >= right_rank,
            "severity sort order must be descending"
        );
        if left_rank == right_rank {
            assert!(
                left.code <= right.code,
                "code sort order must be ascending when severities match"
            );
            if left.code == right.code {
                assert!(
                    left.location <= right.location,
                    "location sort order must be ascending when severity and code match"
                );
            }
        }
    }

    assert_snapshot!(
        serde_json::to_string_pretty(&results).expect("serialize lint results"),
        @r###"
    [
      {
        "code": "WFG-LINT-001",
        "severity": "error",
        "message": "duplicate task id 'start' found 2 times",
        "location": "start",
        "suggestion": "rename tasks so every task id is unique"
      },
      {
        "code": "WFG-LINT-002",
        "severity": "error",
        "message": "transition from 'start' references unknown target 'missing'",
        "location": "start",
        "suggestion": "point transitions to an existing task id"
      },
      {
        "code": "WFG-LINT-004",
        "severity": "error",
        "message": "AssertCompletedOperator in 'done' references unknown task 'ghost'",
        "location": "done",
        "suggestion": "update 'require' to include only valid task ids"
      },
      {
        "code": "WFG-LINT-005",
        "severity": "error",
        "message": "$expr parse failure for '1 +': expression compile error: Script is incomplete (line 1, position 4)",
        "location": "start",
        "suggestion": "fix syntax so the expression compiles"
      },
      {
        "code": "WFG-LINT-006",
        "severity": "error",
        "message": "$expr in transition 'when' for task 'start' does not evaluate to bool",
        "location": "start",
        "suggestion": "ensure transition 'when' expressions return true/false"
      },
      {
        "code": "WFG-LINT-008",
        "severity": "error",
        "message": "CommandOperator uses shell=true but settings.command_operator.allow_shell is not true",
        "location": "start",
        "suggestion": "set settings.command_operator.allow_shell=true to opt in explicitly"
      }
    ]
    "###
    );
}

#[test]
fn shell_opt_in_rule_is_enforced() {
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
      allow_shell: false
  tasks:
    - id: start
      operator: CommandOperator
      params:
        cmd: "echo hello"
        shell: true
"#;

    let file = NamedTempFile::new().expect("temp file");
    fs::write(file.path(), workflow).expect("write workflow");
    let document = schema::parse_workflow(file.path()).expect("parse workflow");

    let results = LintRegistry::new().run(&document);
    assert!(results.iter().any(|result| result.code == "WFG-LINT-008"));
}

fn severity_rank(severity: LintSeverity) -> u8 {
    match severity {
        LintSeverity::Error => 3,
        LintSeverity::Warning => 2,
        LintSeverity::Info => 1,
    }
}
