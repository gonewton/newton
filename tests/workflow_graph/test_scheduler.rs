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

// A2: two transitions from start; lower priority number wins.
const PRIORITY_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: low_prio_target
          priority: 10
          when:
            $expr: "true"
        - to: high_prio_target
          priority: 1
          when:
            $expr: "true"
    - id: low_prio_target
      operator: NoOpOperator
      params: {}
    - id: high_prio_target
      operator: NoOpOperator
      params: {}
"#;

// A5: two tasks in a loop; per-task limit is high but global cap is low.
const GLOBAL_ITER_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: task_a
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 100
    max_workflow_iterations: 3
  tasks:
    - id: task_a
      operator: NoOpOperator
      params: {}
      transitions:
        - to: task_b
          when:
            $expr: "true"
    - id: task_b
      operator: NoOpOperator
      params: {}
      transitions:
        - to: task_a
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

// A3: when branch_a and branch_b both transition to done in the same tick,
// done must be enqueued only once (run_seq == 1).
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

    let summary = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        overrides,
    )
    .await
    .expect("execution succeeded");
    let done = summary
        .completed_tasks
        .get("done")
        .expect("done task recorded");
    assert_eq!(done.run_seq, 1);
}

// A4: self-loop hits per-task iteration cap → WFG-ITER-002.
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

    let result = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        overrides,
    )
    .await;
    let err = result.expect_err("should hit iteration limit");
    assert_eq!(err.code, "WFG-ITER-002");
}

// A2: lower priority number wins when both transitions evaluate to true.
#[tokio::test]
async fn higher_priority_transition_wins() {
    let file = write_workflow(PRIORITY_WORKFLOW);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry(workspace.clone());
    let overrides = executor::ExecutionOverrides {
        parallel_limit: Some(1),
        max_time_seconds: Some(60),
    };

    let summary = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        overrides,
    )
    .await
    .expect("execution succeeded");
    // priority=1 beats priority=10; high_prio_target runs, low_prio_target does not.
    assert!(summary.completed_tasks.contains_key("high_prio_target"));
    assert!(!summary.completed_tasks.contains_key("low_prio_target"));
}

// A5: two tasks in a mutual loop exhaust the global iteration cap → WFG-ITER-001.
#[tokio::test]
async fn workflow_exhausts_global_iteration_limit() {
    let file = write_workflow(GLOBAL_ITER_WORKFLOW);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry(workspace.clone());
    let overrides = executor::ExecutionOverrides {
        parallel_limit: Some(1),
        max_time_seconds: Some(60),
    };

    let result = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        overrides,
    )
    .await;
    let err = result.expect_err("should hit global iteration limit");
    assert_eq!(err.code, "WFG-ITER-001");
}
