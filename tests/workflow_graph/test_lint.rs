use insta::assert_debug_snapshot;
use newton::core::workflow_graph::{lint::LintRegistry, schema::WorkflowDocument};
use serde_yaml::from_str;

fn load_document(yaml: &str) -> WorkflowDocument {
    from_str(yaml).expect("failed to parse workflow YAML")
}

const MULTI_ISSUE_WORKFLOW: &str = r#"
version: "2.0"
mode: "workflow_graph"
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 5
    command_operator:
      allow_shell: false
  tasks:
    - id: start
      operator: CommandOperator
      params:
        cmd: "echo start"
        shell: true
      transitions:
        - to: missing-target
    - id: check
      operator: AssertCompletedOperator
      params:
        require:
          - missing-dependency
      transitions:
        - to: loop-one
    - id: loop-one
      operator: CommandOperator
      transitions:
        - to: loop-two
    - id: loop-two
      operator: CommandOperator
      transitions:
        - to: loop-one
    - id: unreachable
      operator: CommandOperator
      params:
        cmd:
          $expr: "1 + 1"
    - id: when-non-bool
      operator: CommandOperator
      transitions:
        - to: start
          when:
            $expr: "1 + 2"
    - id: parse-error
      operator: CommandOperator
      params:
        message:
          $expr: "invalid +"
"#;

const DUPLICATE_WORKFLOW: &str = r#"
version: "2.0"
mode: "workflow_graph"
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 3
    max_workflow_iterations: 3
  tasks:
    - id: start
      operator: CommandOperator
      params:
        cmd: "echo start"
    - id: start
      operator: CommandOperator
      params:
        cmd: "echo duplicate"
"#;

#[test]
fn lint_generates_stable_diagnostics() {
    let document = load_document(MULTI_ISSUE_WORKFLOW);
    let results = LintRegistry::new().run(&document);
    assert_debug_snapshot!(results);
}

#[test]
fn lint_reports_duplicate_task_ids() {
    let document = load_document(DUPLICATE_WORKFLOW);
    let results = LintRegistry::new().run(&document);
    assert!(
        results.iter().any(|result| result.code == "WFG-LINT-001"),
        "duplicate task id diagnostic was not emitted"
    );
}
