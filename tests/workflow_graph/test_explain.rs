use insta::assert_snapshot;
use newton::core::workflow_graph::{explain, schema::WorkflowDocument};
use serde_json::{json, Value};
use serde_yaml::from_str;

fn load_document(yaml: &str) -> WorkflowDocument {
    from_str(yaml).expect("failed to parse workflow YAML")
}

const EXPLAIN_WORKFLOW: &str = r#"
version: "2.0"
mode: "workflow_graph"
workflow:
  context:
    env: "dev"
    count: 1
  settings:
    entry_task: start
    max_time_seconds: 120
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 2
    max_workflow_iterations: 2
  tasks:
    - id: start
      operator: CommandOperator
      params:
        greeting:
          $expr: "context.env + \"-action\""
        checker:
          $expr: "tasks.next.status == \"success\""
      transitions:
        - to: next
          priority: 10
          when:
            $expr: "context.env != \"staging\""
        - to: other
          priority: 20
          when:
            $expr: "context.count > 0"
    - id: next
      operator: CommandOperator
      params:
        metadata:
          $expr: "{\"region\":\"us\"}"
      transitions:
        - to: start
          priority: 5
    - id: other
      operator: CommandOperator
      params:
        cwd: "/workspace"
"#;

#[test]
fn explain_output_matches_goldens() {
    let document = load_document(EXPLAIN_WORKFLOW);
    let overrides = vec![
        ("env".to_string(), Value::String("prod".to_string())),
        ("count".to_string(), json!(5)),
    ];
    let output = explain::build_explain_output(&document, &overrides);
    let serialized = serde_json::to_string_pretty(&output).expect("serialize explain output");
    assert_snapshot!(serialized);
}
