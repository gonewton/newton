use newton::core::workflow_graph::{executor, operator::OperatorRegistry, operators, schema};
use std::io::Write;
use tempfile::NamedTempFile;

const DEDUPE_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 60
    parallel_limit: 2
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: branch_a
          when:
            $expr: "true"
        - to: branch_b
          when:
            $expr: "true"
    - id: branch_a
      operator: NoOpOperator
      params: {}
      transitions:
        - to: done
          when:
            $expr: "true"
    - id: branch_b
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

const LOOP_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: loop_task
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 1
    max_workflow_iterations: 10
  tasks:
    - id: loop_task
      operator: NoOpOperator
      params: {}
      transitions:
        - to: loop_task
          when:
            $expr: "true"
"#;

fn build_registry(workspace: std::path::PathBuf) -> OperatorRegistry {
    let mut builder = OperatorRegistry::builder();
    operators::register_builtins(&mut builder, workspace);
    builder.build()
}

fn write_workflow(yaml: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("temp file");
    write!(file, "{}", yaml).unwrap();
    file
}

#[tokio::test]
async fn transitions_deduplicate_targets_per_tick() {
    let file = write_workflow(DEDUPE_WORKFLOW);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry(workspace.clone());
    let overrides = executor::ExecutionOverrides {
        parallel_limit: Some(2),
        max_time_seconds: Some(60),
    };

    let summary = executor::execute_workflow(document, registry, workspace, overrides)
        .await
        .expect("execution succeeded");
    let done = summary
        .completed_tasks
        .get("done")
        .expect("done task recorded");
    assert_eq!(done.run_seq, 1);
}

#[tokio::test]
async fn loop_exhausts_iteration_limit() {
    let file = write_workflow(LOOP_WORKFLOW);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry(workspace.clone());
    let overrides = executor::ExecutionOverrides {
        parallel_limit: Some(1),
        max_time_seconds: Some(60),
    };

    let result = executor::execute_workflow(document, registry, workspace, overrides).await;
    let err = result.expect_err("should hit iteration limit");
    assert_eq!(err.code, "WFG-ITER-002");
}
