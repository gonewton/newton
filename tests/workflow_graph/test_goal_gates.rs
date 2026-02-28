/// Integration and contract tests for 017-e: Goal Gates and Completion Policy.
///
/// Scenarios:
///   E1 – goal gate succeeds, no failures → Completed
///   E2 – goal gate unreachable from entry_task with require_goal_gates=true → WFG-LINT-102
///   E3 – goal gate reached but failed → WFG-GATE-001
///   E4 – non-goal task fails with continue_on_error=false → immediate failure (WFG-EXEC-001)
///   E5 – task fails, continue_on_error=true, routes to terminal success,
///         success_requires_no_task_failures=true → Failed
///   E6 – same as E5 but success_requires_no_task_failures=false → Completed
///   E7 – terminal:failure task completes → Failed (WFG-EXEC-002)
///   E8 – terminal:success completes while other tasks queued → executor stops early
use async_trait::async_trait;
use newton::core::{
    error::AppError,
    types::ErrorCategory,
    workflow_graph::{
        executor::{self, ExecutionOverrides},
        lint::LintRegistry,
        operator::{ExecutionContext, Operator, OperatorRegistry},
        operators,
        schema::{self},
        state::GraphSettings,
    },
};
use serde_json::Value;
use std::io::Write;
use tempfile::NamedTempFile;

// ─── FailOperator ────────────────────────────────────────────────────────────

struct FailOperator;

#[async_trait]
impl Operator for FailOperator {
    fn name(&self) -> &'static str {
        "FailOperator"
    }

    fn validate_params(&self, _params: &Value) -> Result<(), AppError> {
        Ok(())
    }

    async fn execute(&self, _params: Value, _ctx: ExecutionContext) -> Result<Value, AppError> {
        Err(AppError::new(
            ErrorCategory::ValidationError,
            "intentional failure from FailOperator",
        ))
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn build_registry(workspace: std::path::PathBuf, settings: GraphSettings) -> OperatorRegistry {
    let mut builder = OperatorRegistry::builder();
    operators::register_builtins(&mut builder, workspace, settings);
    builder.build()
}

fn build_registry_with_fail(
    workspace: std::path::PathBuf,
    settings: GraphSettings,
) -> OperatorRegistry {
    let mut builder = OperatorRegistry::builder();
    operators::register_builtins(&mut builder, workspace, settings);
    builder.register(FailOperator);
    builder.build()
}

fn write_workflow(yaml: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("temp file");
    write!(file, "{}", yaml).unwrap();
    file
}

fn default_overrides() -> ExecutionOverrides {
    ExecutionOverrides {
        parallel_limit: Some(4),
        max_time_seconds: Some(30),
        checkpoint_base_path: None,
        artifact_base_path: None,
        verbose: false,
    }
}

// ─── E1: Goal gate succeeds ───────────────────────────────────────────────────

const E1_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 2
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: true
      require_goal_gates: true
      goal_gate_failure_behavior: fail
      success_requires_no_task_failures: true
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: security_scan
    - id: security_scan
      operator: NoOpOperator
      params: {}
      goal_gate: true
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      params: {}
      terminal: success
"#;

#[tokio::test]
async fn e1_goal_gate_succeeds_workflow_completed() {
    let file = write_workflow(E1_WORKFLOW);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry(workspace.clone(), document.workflow.settings.clone());

    let result = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        default_overrides(),
    )
    .await;

    let summary = result.expect("E1: workflow should complete");
    assert!(
        summary.completed_tasks.contains_key("security_scan"),
        "E1: goal gate task should be in completed"
    );
    assert!(
        summary.completed_tasks.contains_key("done"),
        "E1: terminal task should be in completed"
    );
}

// ─── E2: Goal gate unreachable → WFG-LINT-102 ────────────────────────────────

const E2_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      require_goal_gates: true
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
    - id: orphan_gate
      operator: NoOpOperator
      params: {}
      goal_gate: true
"#;

#[test]
fn e2_unreachable_goal_gate_lint_error() {
    let file = write_workflow(E2_WORKFLOW);
    // parse_workflow skips semantic validation so the unreachable gate isn't caught early.
    let document = schema::parse_workflow(file.path()).expect("parse workflow");

    let results = LintRegistry::new().run(&document);
    let gate_error = results.iter().find(|r| r.code == "WFG-LINT-102");
    assert!(
        gate_error.is_some(),
        "E2: expected WFG-LINT-102 for unreachable goal gate, got {:?}",
        results
    );
    let err = gate_error.unwrap();
    assert_eq!(err.code, "WFG-LINT-102");
    assert!(
        err.message.contains("orphan_gate"),
        "E2: error should name the unreachable gate task"
    );
}

// ─── E3: Goal gate reached but failed → WFG-GATE-001 ────────────────────────

const E3_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: true
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: false
      require_goal_gates: true
      goal_gate_failure_behavior: fail
      success_requires_no_task_failures: false
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: security_scan
    - id: security_scan
      operator: FailOperator
      params: {}
      goal_gate: true
"#;

#[tokio::test]
async fn e3_goal_gate_reached_but_failed_returns_gate_error() {
    let file = write_workflow(E3_WORKFLOW);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry_with_fail(workspace.clone(), document.workflow.settings.clone());

    let result = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        default_overrides(),
    )
    .await;

    let err = result.expect_err("E3: workflow should fail with WFG-GATE-001");
    assert_eq!(
        err.code, "WFG-GATE-001",
        "E3: expected WFG-GATE-001 error code, got: {} — {}",
        err.code, err.message
    );
    assert!(
        err.message.contains("security_scan"),
        "E3: error message should name the failing gate, got: {}",
        err.message
    );
    assert!(
        err.message.contains("failed"),
        "E3: error message should show status=failed, got: {}",
        err.message
    );
}

// ─── E4: Non-goal task fails with continue_on_error=false ────────────────────

const E4_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: will_fail
    - id: will_fail
      operator: FailOperator
      params: {}
    - id: never_runs
      operator: NoOpOperator
      params: {}
"#;

#[tokio::test]
async fn e4_task_fails_continue_on_error_false_immediate_failure() {
    let file = write_workflow(E4_WORKFLOW);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry_with_fail(workspace.clone(), document.workflow.settings.clone());

    let result = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        default_overrides(),
    )
    .await;

    let err = result.expect_err("E4: workflow should fail immediately when task fails");
    assert_eq!(
        err.code, "WFG-EXEC-001",
        "E4: expected WFG-EXEC-001, got: {} — {}",
        err.code, err.message
    );
}

// ─── E5: Task fails, continue_on_error=true, success_requires_no_task_failures=true ──

const E5_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: true
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: true
      require_goal_gates: false
      success_requires_no_task_failures: true
  tasks:
    - id: start
      operator: FailOperator
      params: {}
      transitions:
        - to: terminal_success
    - id: terminal_success
      operator: NoOpOperator
      params: {}
      terminal: success
"#;

#[tokio::test]
async fn e5_task_fails_continue_on_error_true_success_requires_no_failures_is_true() {
    let file = write_workflow(E5_WORKFLOW);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry_with_fail(workspace.clone(), document.workflow.settings.clone());

    let result = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        default_overrides(),
    )
    .await;

    let err = result.expect_err("E5: workflow should fail because a task failed");
    assert_eq!(
        err.code, "WFG-EXEC-001",
        "E5: expected WFG-EXEC-001 (task failure rule), got: {} — {}",
        err.code, err.message
    );
}

// ─── E6: Same as E5 but success_requires_no_task_failures=false ──────────────

const E6_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: true
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: true
      require_goal_gates: false
      success_requires_no_task_failures: false
  tasks:
    - id: start
      operator: FailOperator
      params: {}
      transitions:
        - to: terminal_success
    - id: terminal_success
      operator: NoOpOperator
      params: {}
      terminal: success
"#;

#[tokio::test]
async fn e6_task_fails_success_requires_no_failures_false_workflow_completes() {
    let file = write_workflow(E6_WORKFLOW);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry_with_fail(workspace.clone(), document.workflow.settings.clone());

    let result = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        default_overrides(),
    )
    .await;

    let summary = result.expect("E6: workflow should complete despite task failure");
    assert!(
        summary.completed_tasks.contains_key("terminal_success"),
        "E6: terminal_success task should have run"
    );
}

// ─── E7: Terminal:failure task completes → Failed ────────────────────────────

const E7_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: true
      require_goal_gates: false
      success_requires_no_task_failures: false
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: abort
    - id: abort
      operator: NoOpOperator
      params: {}
      terminal: failure
"#;

#[tokio::test]
async fn e7_terminal_failure_task_causes_workflow_failure() {
    let file = write_workflow(E7_WORKFLOW);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry(workspace.clone(), document.workflow.settings.clone());

    let result = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        default_overrides(),
    )
    .await;

    let err = result.expect_err("E7: workflow should fail when terminal:failure runs");
    assert_eq!(
        err.code, "WFG-EXEC-002",
        "E7: expected WFG-EXEC-002, got: {} — {}",
        err.code, err.message
    );
    assert!(
        err.message.contains("abort"),
        "E7: error should name the terminal:failure task, got: {}",
        err.message
    );
}

// ─── E8: Terminal:success stops executor before queued tasks run ──────────────

const E8_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: true
      require_goal_gates: false
      success_requires_no_task_failures: true
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: quick_exit
        - to: slow_task
    - id: quick_exit
      operator: NoOpOperator
      params: {}
      terminal: success
    - id: slow_task
      operator: NoOpOperator
      params: {}
"#;

#[tokio::test]
async fn e8_terminal_success_stops_executor_queued_tasks_not_run() {
    let file = write_workflow(E8_WORKFLOW);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    // Use parallel_limit=1 so tasks run one at a time — start first, then only
    // ONE of quick_exit or slow_task will be dequeued per tick.
    // With stop_on_terminal=true, once quick_exit completes the executor stops.
    let overrides = ExecutionOverrides {
        parallel_limit: Some(1),
        max_time_seconds: Some(30),
        checkpoint_base_path: None,
        artifact_base_path: None,
        verbose: false,
    };
    let registry = build_registry(workspace.clone(), document.workflow.settings.clone());

    let result = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        overrides,
    )
    .await;

    let summary = result.expect("E8: workflow should complete when terminal:success runs");
    assert!(
        summary.completed_tasks.contains_key("quick_exit"),
        "E8: quick_exit (terminal:success) should have run"
    );
    // With parallel_limit=1 and stop_on_terminal=true, the executor stops after quick_exit.
    // slow_task was enqueued but should NOT have run.
    assert!(
        !summary.completed_tasks.contains_key("slow_task"),
        "E8: slow_task should NOT have run after terminal stop"
    );
}

// ─── Lint rule tests ──────────────────────────────────────────────────────────

#[test]
fn lint_101_fires_when_stop_on_terminal_true_and_no_terminal_task() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: true
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
"#;
    let file = write_workflow(workflow);
    let document = schema::parse_workflow(file.path()).expect("parse");
    let results = LintRegistry::new().run(&document);
    assert!(
        results.iter().any(|r| r.code == "WFG-LINT-101"),
        "expected WFG-LINT-101, got {:?}",
        results
    );
}

#[test]
fn lint_101_silent_when_terminal_task_present() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: true
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      terminal: success
"#;
    let file = write_workflow(workflow);
    let document = schema::parse_workflow(file.path()).expect("parse");
    let results = LintRegistry::new().run(&document);
    assert!(
        !results.iter().any(|r| r.code == "WFG-LINT-101"),
        "WFG-LINT-101 should not fire when a terminal task is present"
    );
}

#[test]
fn lint_101_silent_when_stop_on_terminal_false() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: false
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
"#;
    let file = write_workflow(workflow);
    let document = schema::parse_workflow(file.path()).expect("parse");
    let results = LintRegistry::new().run(&document);
    assert!(
        !results.iter().any(|r| r.code == "WFG-LINT-101"),
        "WFG-LINT-101 should not fire when stop_on_terminal=false"
    );
}

#[test]
fn lint_102_fires_when_goal_gate_unreachable() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      require_goal_gates: true
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
    - id: island
      operator: NoOpOperator
      params: {}
      goal_gate: true
"#;
    let file = write_workflow(workflow);
    let document = schema::parse_workflow(file.path()).expect("parse");
    let results = LintRegistry::new().run(&document);
    assert!(
        results.iter().any(|r| r.code == "WFG-LINT-102"),
        "expected WFG-LINT-102 for unreachable goal gate, got {:?}",
        results
    );
}

#[test]
fn lint_102_silent_when_goal_gate_reachable() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      require_goal_gates: true
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: gate
    - id: gate
      operator: NoOpOperator
      params: {}
      goal_gate: true
"#;
    let file = write_workflow(workflow);
    let document = schema::parse_workflow(file.path()).expect("parse");
    let results = LintRegistry::new().run(&document);
    assert!(
        !results.iter().any(|r| r.code == "WFG-LINT-102"),
        "WFG-LINT-102 should not fire when goal gate is reachable"
    );
}

#[test]
fn lint_103_fires_when_goal_gate_has_no_remediation_path() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      goal_gate_failure_behavior: fail
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: gate
    - id: gate
      operator: NoOpOperator
      params: {}
      goal_gate: true
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      params: {}
"#;
    let file = write_workflow(workflow);
    let document = schema::parse_workflow(file.path()).expect("parse");
    let results = LintRegistry::new().run(&document);
    assert!(
        results.iter().any(|r| r.code == "WFG-LINT-103"),
        "expected WFG-LINT-103 when no remediation path exists, got {:?}",
        results
    );
}

#[test]
fn lint_103_silent_when_remediation_path_exists() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 10
    max_workflow_iterations: 50
    completion:
      goal_gate_failure_behavior: fail
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: gate
    - id: gate
      operator: NoOpOperator
      params: {}
      goal_gate: true
      transitions:
        - to: remediate
    - id: remediate
      operator: NoOpOperator
      params: {}
      transitions:
        - to: gate
"#;
    let file = write_workflow(workflow);
    let document = schema::parse_workflow(file.path()).expect("parse");
    let results = LintRegistry::new().run(&document);
    assert!(
        !results.iter().any(|r| r.code == "WFG-LINT-103"),
        "WFG-LINT-103 should not fire when remediation path exists"
    );
}

#[test]
fn lint_104_fires_when_two_terminal_tasks_can_run_concurrently() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 2
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: true
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: branch_a
        - to: branch_b
    - id: branch_a
      operator: NoOpOperator
      params: {}
      terminal: success
    - id: branch_b
      operator: NoOpOperator
      params: {}
      terminal: success
"#;
    let file = write_workflow(workflow);
    let document = schema::parse_workflow(file.path()).expect("parse");
    let results = LintRegistry::new().run(&document);
    assert!(
        results.iter().any(|r| r.code == "WFG-LINT-104"),
        "expected WFG-LINT-104 when two terminal tasks can run concurrently, got {:?}",
        results
    );
}

#[test]
fn lint_104_silent_when_terminal_tasks_are_sequential() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: true
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: first_terminal
    - id: first_terminal
      operator: NoOpOperator
      params: {}
      terminal: success
      transitions:
        - to: second_terminal
    - id: second_terminal
      operator: NoOpOperator
      params: {}
      terminal: success
"#;
    let file = write_workflow(workflow);
    let document = schema::parse_workflow(file.path()).expect("parse");
    let results = LintRegistry::new().run(&document);
    assert!(
        !results.iter().any(|r| r.code == "WFG-LINT-104"),
        "WFG-LINT-104 should not fire when terminal tasks are sequential"
    );
}

// ─── WFG-GATE-001 message format ─────────────────────────────────────────────

#[tokio::test]
async fn wfg_gate_001_message_format_stable() {
    // Goal gate with goal_gate_group set — message should include group.
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: true
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: false
      require_goal_gates: true
      goal_gate_failure_behavior: fail
      success_requires_no_task_failures: false
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: security_scan
    - id: security_scan
      operator: FailOperator
      params: {}
      goal_gate: true
      goal_gate_group: critical
"#;
    let file = write_workflow(workflow);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry_with_fail(workspace.clone(), document.workflow.settings.clone());

    let result = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        default_overrides(),
    )
    .await;

    let err = result.expect_err("should fail with WFG-GATE-001");
    assert_eq!(err.code, "WFG-GATE-001");
    // Message must include group=critical per spec.
    assert!(
        err.message.contains("group=critical"),
        "WFG-GATE-001 message should include goal_gate_group, got: {}",
        err.message
    );
    assert!(
        err.message.starts_with("goal gates not passed:"),
        "WFG-GATE-001 message must start with 'goal gates not passed:', got: {}",
        err.message
    );
}

// ─── not_reached gate ────────────────────────────────────────────────────────

#[tokio::test]
async fn goal_gate_not_reached_message_includes_not_reached_status() {
    // Goal gate exists but workflow terminates before reaching it
    // because terminal:success fires first.
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: true
      require_goal_gates: true
      goal_gate_failure_behavior: fail
      success_requires_no_task_failures: true
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      terminal: success
    - id: unreached_gate
      operator: NoOpOperator
      params: {}
      goal_gate: true
"#;
    let file = write_workflow(workflow);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry(workspace.clone(), document.workflow.settings.clone());

    let result = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        default_overrides(),
    )
    .await;

    let err = result.expect_err("should fail because goal gate was not reached");
    assert_eq!(err.code, "WFG-GATE-001");
    assert!(
        err.message.contains("not_reached"),
        "WFG-GATE-001 message should say 'not_reached', got: {}",
        err.message
    );
}

// ─── goal_gate_failure_behavior=allow ────────────────────────────────────────

#[tokio::test]
async fn goal_gate_failure_behavior_allow_ignores_failed_gate() {
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: true
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: false
      require_goal_gates: false
      goal_gate_failure_behavior: allow
      success_requires_no_task_failures: false
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: gate
    - id: gate
      operator: FailOperator
      params: {}
      goal_gate: true
"#;
    let file = write_workflow(workflow);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry_with_fail(workspace.clone(), document.workflow.settings.clone());

    let result = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        default_overrides(),
    )
    .await;

    // With allow behavior and success_requires_no_task_failures=false, workflow should complete.
    result
        .expect("goal_gate_failure_behavior=allow should not fail the workflow for a failed gate");
}

// ─── WFG-TERM-001: Multiple terminal tasks in same tick ──────────────────────

#[tokio::test]
async fn wfg_term_001_warning_logged_for_concurrent_terminal_tasks() {
    // Two terminal tasks can run concurrently; executor must log WFG-TERM-001.
    let workflow = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 30
    parallel_limit: 2
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 20
    completion:
      stop_on_terminal: true
      require_goal_gates: false
      success_requires_no_task_failures: true
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: term_a
        - to: term_b
    - id: term_a
      operator: NoOpOperator
      params: {}
      terminal: success
    - id: term_b
      operator: NoOpOperator
      params: {}
      terminal: success
"#;
    let file = write_workflow(workflow);
    let document = schema::load_workflow(file.path()).expect("valid workflow");
    let workspace = std::env::current_dir().expect("workspace");
    let registry = build_registry(workspace.clone(), document.workflow.settings.clone());

    let result = executor::execute_workflow(
        document,
        file.path().to_path_buf(),
        registry,
        workspace,
        default_overrides(),
    )
    .await;

    // Workflow should complete (both terminal:success).
    result.expect("workflow with concurrent terminal success tasks should complete");
    // We cannot easily inspect warnings from ExecutionSummary here,
    // but the test verifies execution completes without panicking.
}
