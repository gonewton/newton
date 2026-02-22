use newton::core::workflow_graph::schema;
use std::fs;
use tempfile::NamedTempFile;

const VALID_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 2
    max_workflow_iterations: 5
  tasks:
    - id: start
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

const INVALID_TRANSITION: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 2
    max_workflow_iterations: 5
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: missing
          when:
            $expr: "true"
"#;

#[test]
fn valid_workflow_parses_and_validates() {
    let file = NamedTempFile::new().expect("temp file");
    let path = file.path().to_owned();
    drop(file);
    fs::write(&path, VALID_WORKFLOW).unwrap();
    let workflow = schema::load_workflow(&path);
    assert!(workflow.is_ok());
}

#[test]
fn invalid_transition_reports_error() {
    let file = NamedTempFile::new().expect("temp file");
    let path = file.path().to_owned();
    drop(file);
    fs::write(&path, INVALID_TRANSITION).unwrap();
    let workflow = schema::load_workflow(&path);
    assert!(workflow.is_err());
    let err = workflow.err().unwrap();
    assert!(err.message.contains("unknown task"));
}
