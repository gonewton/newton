//! ADR-0014: descriptor/execution split — executing a described-but-unwired
//! operator (a loop operator with no BackendStore in this context) must fail
//! with a clear WFG-OP-002 error, distinct from WFG-OP-001 ("operator is not
//! registered" for a genuinely unknown name / typo).
use newton_core::workflow::{
    executor::{self, ExecutionOverrides},
    operator::OperatorRegistry,
    operators, schema, state,
};
use std::io::Write;
use std::path::PathBuf;
use tempfile::{tempdir, NamedTempFile};

const GRADER_COMMAND_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: grade
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: grade
      operator: GraderCommandOperator
      params:
        cmd: "true"
        grader: "test-grader"
        scope: "repo"
        scope_id: "test"
"#;

fn build_registry_without_store(
    workspace: PathBuf,
    settings: state::GraphSettings,
) -> OperatorRegistry {
    let mut builder = OperatorRegistry::builder();
    // No BackendStore wired — mirrors `newton schema export`'s registry and
    // any other caller that never opens the state store.
    operators::register_builtins(&mut builder, workspace, settings);
    builder.build()
}

fn write_workflow(yaml: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("temp file");
    write!(file, "{}", yaml).unwrap();
    file
}

#[tokio::test]
async fn executing_loop_operator_without_store_fails_with_clear_error() {
    let workspace = tempdir().expect("workspace");
    let workflow_file = write_workflow(GRADER_COMMAND_WORKFLOW);
    let document = schema::load_workflow(workflow_file.path()).expect("valid workflow");
    let settings = document.workflow.settings.clone();

    let registry = build_registry_without_store(workspace.path().to_path_buf(), settings);

    // Confirm the ADR-0014 premise up front: the operator IS described...
    assert!(registry.is_described("GraderCommandOperator"));
    // ...but has no executable instance without a BackendStore.
    assert!(registry.get("GraderCommandOperator").is_none());

    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        sink: None,
        pre_seed_nodes: true,
        state_dir: None,
    };

    let result = executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry,
        workspace.path().to_path_buf(),
        overrides,
    )
    .await;

    let err = result.expect_err("execution must fail: operator has no wired executable");
    assert_eq!(
        err.code, "WFG-OP-002",
        "described-but-unwired operator must use the WFG-OP-002 diagnostic, not vanish silently"
    );
    assert!(
        err.message.contains("backend store"),
        "error message must explain the operator requires a backend store, got: {}",
        err.message
    );
}
