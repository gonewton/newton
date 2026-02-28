use async_trait::async_trait;
use chrono::Utc;
use newton::core::error::AppError;
use newton::core::types::ErrorCategory;
use newton::core::workflow_graph::executor::{self, ExecutionOverrides, ExecutionSummary};
use newton::core::workflow_graph::human::{
    ApprovalDefault, ApprovalResult, DecisionResult, Interviewer,
};
use newton::core::workflow_graph::operator::OperatorRegistry;
use newton::core::workflow_graph::operators::command::{
    CommandExecutionOutput, CommandExecutionRequest, CommandRunner,
};
use newton::core::workflow_graph::operators::{self, BuiltinOperatorDeps};
use newton::core::workflow_graph::schema::{self, TriggerType, WorkflowTrigger};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::{tempdir, NamedTempFile};
use tokio::time::{sleep, Duration};

#[derive(Clone)]
enum MockCommandStep {
    Success {
        stdout: &'static str,
        stderr: &'static str,
        exit_code: i32,
    },
    Error {
        code: &'static str,
        message: &'static str,
    },
    DelaySuccess {
        delay_ms: u64,
        stdout: &'static str,
        stderr: &'static str,
        exit_code: i32,
    },
}

#[derive(Clone)]
struct MockCommandRunner {
    plans: Arc<Mutex<HashMap<String, VecDeque<MockCommandStep>>>>,
}

impl MockCommandRunner {
    fn new(plans: HashMap<String, VecDeque<MockCommandStep>>) -> Self {
        Self {
            plans: Arc::new(Mutex::new(plans)),
        }
    }
}

#[async_trait]
impl CommandRunner for MockCommandRunner {
    async fn run(
        &self,
        request: &CommandExecutionRequest,
    ) -> Result<CommandExecutionOutput, AppError> {
        let step = {
            let mut guard = self.plans.lock().expect("lock command plans");
            guard
                .get_mut(request.cmd.trim())
                .and_then(VecDeque::pop_front)
                .unwrap_or(MockCommandStep::Success {
                    stdout: "",
                    stderr: "",
                    exit_code: 0,
                })
        };

        match step {
            MockCommandStep::Success {
                stdout,
                stderr,
                exit_code,
            } => Ok(CommandExecutionOutput {
                stdout: stdout.as_bytes().to_vec(),
                stderr: stderr.as_bytes().to_vec(),
                exit_code,
            }),
            MockCommandStep::Error { code, message } => {
                Err(AppError::new(ErrorCategory::ToolExecutionError, message).with_code(code))
            }
            MockCommandStep::DelaySuccess {
                delay_ms,
                stdout,
                stderr,
                exit_code,
            } => {
                sleep(Duration::from_millis(delay_ms)).await;
                Ok(CommandExecutionOutput {
                    stdout: stdout.as_bytes().to_vec(),
                    stderr: stderr.as_bytes().to_vec(),
                    exit_code,
                })
            }
        }
    }
}

#[derive(Clone)]
struct FakeInterviewer {
    approval_result: ApprovalResult,
    decision_result: DecisionResult,
}

impl FakeInterviewer {
    fn approve_and_choose(choice: &str) -> Self {
        Self {
            approval_result: ApprovalResult {
                approved: true,
                reason: "approved by test".to_string(),
                timestamp: Utc::now(),
                timeout_applied: false,
                default_used: false,
            },
            decision_result: DecisionResult {
                choice: choice.to_string(),
                timestamp: Utc::now(),
                timeout_applied: false,
                default_used: false,
                response_text: Some("1".to_string()),
            },
        }
    }
}

#[async_trait]
impl Interviewer for FakeInterviewer {
    fn interviewer_type(&self) -> &'static str {
        "fake-matrix"
    }

    async fn ask_approval(
        &self,
        _prompt: &str,
        _timeout: Option<Duration>,
        _default_on_timeout: Option<ApprovalDefault>,
    ) -> Result<ApprovalResult, AppError> {
        Ok(self.approval_result.clone())
    }

    async fn ask_choice(
        &self,
        _prompt: &str,
        _choices: &[String],
        _timeout: Option<Duration>,
        _default_choice: Option<&str>,
    ) -> Result<DecisionResult, AppError> {
        Ok(self.decision_result.clone())
    }
}

fn build_registry(
    workspace: PathBuf,
    settings: newton::core::workflow_graph::state::GraphSettings,
    deps: BuiltinOperatorDeps,
) -> OperatorRegistry {
    let mut builder = OperatorRegistry::builder();
    operators::register_builtins_with_deps(&mut builder, workspace, settings, deps);
    builder.build()
}

async fn execute_yaml(
    workspace: &Path,
    yaml: &str,
    deps: BuiltinOperatorDeps,
    trigger_payload: Option<Value>,
) -> Result<ExecutionSummary, AppError> {
    let mut workflow_file = NamedTempFile::new().expect("workflow temp file");
    write!(workflow_file, "{yaml}").expect("write workflow");
    let mut document = schema::load_workflow(workflow_file.path()).expect("load workflow");
    if let Some(payload) = trigger_payload {
        document.triggers = Some(WorkflowTrigger {
            trigger_type: TriggerType::Manual,
            schema_version: "1".to_string(),
            payload,
        });
    }
    let settings = document.workflow.settings.clone();
    let registry = build_registry(workspace.to_path_buf(), settings, deps);
    executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry,
        workspace.to_path_buf(),
        ExecutionOverrides {
            parallel_limit: None,
            max_time_seconds: None,
            checkpoint_base_path: None,
            artifact_base_path: None,
            verbose: false,
        },
    )
    .await
}

fn scenario_err(name: &str, msg: impl Into<String>) -> String {
    format!("{name}: {}", msg.into())
}

#[tokio::test]
async fn comprehensive_workflow_matrix_covers_builtin_operators_and_complexities() {
    let scenarios = [
        "basic_single_noop_success",
        "set_context_and_expression_branch",
        "command_success_path",
        "command_retry_after_timeout",
        "read_control_from_trigger_path",
        "assert_completed_pass",
        "assert_completed_missing_dependency_fails",
        "human_approval_and_decision_path",
        "priority_branching",
        "command_execution_error_fails_workflow",
    ];

    let mut failures = Vec::new();
    for scenario in scenarios {
        if let Err(err) = run_scenario(scenario).await {
            failures.push(err);
        }
    }

    assert!(
        failures.is_empty(),
        "scenario failures:\n{}",
        failures.join("\n")
    );
}

async fn run_scenario(name: &str) -> Result<(), String> {
    match name {
        "basic_single_noop_success" => scenario_basic_single_noop_success().await,
        "set_context_and_expression_branch" => scenario_set_context_and_expression_branch().await,
        "command_success_path" => scenario_command_success_path().await,
        "command_retry_after_timeout" => scenario_command_retry_after_timeout().await,
        "read_control_from_trigger_path" => scenario_read_control_from_trigger_path().await,
        "assert_completed_pass" => scenario_assert_completed_pass().await,
        "assert_completed_missing_dependency_fails" => {
            scenario_assert_completed_missing_dependency_fails().await
        }
        "human_approval_and_decision_path" => scenario_human_approval_and_decision_path().await,
        "priority_branching" => scenario_priority_branching().await,
        "command_execution_error_fails_workflow" => {
            scenario_command_execution_error_fails_workflow().await
        }
        _ => Err(format!("unknown scenario {name}")),
    }
}

const SCENARIO_SET_CONTEXT_AND_EXPRESSION_BRANCH_YAML: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: init
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: init
      operator: SetContextOperator
      params:
        patch:
          flag: 1
      transitions:
        - to: route
    - id: route
      operator: NoOpOperator
      params: {}
      transitions:
        - to: success
          when:
            $expr: "context.flag == 1"
        - to: failure
    - id: success
      operator: NoOpOperator
      terminal: success
      params: {}
    - id: failure
      operator: NoOpOperator
      terminal: failure
      params: {}
"#;

const SCENARIO_COMMAND_SUCCESS_PATH_YAML: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: run
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: run
      operator: CommandOperator
      params:
        cmd: "ok_cmd"
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
"#;

const SCENARIO_COMMAND_RETRY_AFTER_TIMEOUT_YAML: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: run
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: run
      operator: CommandOperator
      timeout_ms: 10
      retry:
        max_attempts: 2
        backoff_ms: 0
      params:
        cmd: "flaky"
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
"#;

const SCENARIO_HUMAN_APPROVAL_AND_DECISION_PATH_YAML: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: approval
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: approval
      operator: HumanApprovalOperator
      params:
        prompt: "Ship this?"
      transitions:
        - to: decision
          when:
            $expr: "tasks.approval.output.approved == true"
        - to: fail
    - id: decision
      operator: HumanDecisionOperator
      params:
        prompt: "Choose outcome"
        choices: ["ship", "hold"]
      transitions:
        - to: done
          when:
            $expr: "tasks.decision.output.choice == \"ship\""
        - to: fail
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
    - id: fail
      operator: NoOpOperator
      terminal: failure
      params: {}
"#;

const SCENARIO_PRIORITY_BRANCHING_YAML: &str = r#"
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
    max_workflow_iterations: 10
  tasks:
    - id: start
      operator: NoOpOperator
      params: {}
      transitions:
        - to: low
          priority: 10
          when:
            $expr: "true"
        - to: high
          priority: 1
          when:
            $expr: "true"
    - id: low
      operator: NoOpOperator
      terminal: success
      params: {}
    - id: high
      operator: NoOpOperator
      terminal: success
      params: {}
"#;

fn scenario_workspace(name: &str) -> Result<tempfile::TempDir, String> {
    tempdir().map_err(|err| scenario_err(name, err.to_string()))
}

async fn run_yaml_scenario(
    name: &str,
    workspace: &tempfile::TempDir,
    yaml: &str,
    deps: BuiltinOperatorDeps,
    trigger_payload: Option<Value>,
) -> Result<ExecutionSummary, String> {
    execute_yaml(workspace.path(), yaml, deps, trigger_payload)
        .await
        .map_err(|err| scenario_err(name, err.to_string()))
}

fn expect_task_present(summary: &ExecutionSummary, name: &str, task: &str) -> Result<(), String> {
    if summary.completed_tasks.contains_key(task) {
        return Ok(());
    }
    Err(scenario_err(
        name,
        format!("expected {task} task to complete"),
    ))
}

fn expect_task_absent(summary: &ExecutionSummary, name: &str, task: &str) -> Result<(), String> {
    if summary.completed_tasks.contains_key(task) {
        return Err(scenario_err(
            name,
            format!("{task} task should not complete"),
        ));
    }
    Ok(())
}

fn task_output<'a>(
    summary: &'a ExecutionSummary,
    name: &str,
    task: &str,
) -> Result<&'a Value, String> {
    summary
        .completed_tasks
        .get(task)
        .map(|result| &result.output)
        .ok_or_else(|| scenario_err(name, format!("missing {task} task result")))
}

fn command_deps(plans: HashMap<String, VecDeque<MockCommandStep>>) -> BuiltinOperatorDeps {
    BuiltinOperatorDeps {
        interviewer: None,
        command_runner: Some(Arc::new(MockCommandRunner::new(plans))),
        engine_registry: None,
    }
}

async fn scenario_basic_single_noop_success() -> Result<(), String> {
    const NAME: &str = "basic_single_noop_success";
    let workspace = tempdir().map_err(|err| scenario_err(NAME, err.to_string()))?;
    let summary = execute_yaml(
        workspace.path(),
        r#"
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
    max_workflow_iterations: 10
  tasks:
    - id: start
      operator: NoOpOperator
      terminal: success
      params: {}
"#,
        BuiltinOperatorDeps::default(),
        None,
    )
    .await
    .map_err(|err| scenario_err(NAME, err.to_string()))?;
    if !summary.completed_tasks.contains_key("start") {
        return Err(scenario_err(NAME, "expected start task to complete"));
    }
    Ok(())
}

async fn scenario_set_context_and_expression_branch() -> Result<(), String> {
    const NAME: &str = "set_context_and_expression_branch";
    let workspace = scenario_workspace(NAME)?;
    let summary = run_yaml_scenario(
        NAME,
        &workspace,
        SCENARIO_SET_CONTEXT_AND_EXPRESSION_BRANCH_YAML,
        BuiltinOperatorDeps::default(),
        None,
    )
    .await?;
    expect_task_present(&summary, NAME, "success")?;
    expect_task_absent(&summary, NAME, "failure")
}

async fn scenario_command_success_path() -> Result<(), String> {
    const NAME: &str = "command_success_path";
    let workspace = scenario_workspace(NAME)?;
    let mut plans = HashMap::new();
    plans.insert(
        "ok_cmd".to_string(),
        VecDeque::from([MockCommandStep::Success {
            stdout: "hello",
            stderr: "",
            exit_code: 0,
        }]),
    );
    let summary = run_yaml_scenario(
        NAME,
        &workspace,
        SCENARIO_COMMAND_SUCCESS_PATH_YAML,
        command_deps(plans),
        None,
    )
    .await?;
    if task_output(&summary, NAME, "run")?["stdout"] != "hello" {
        return Err(scenario_err(NAME, "unexpected command stdout"));
    }
    Ok(())
}

async fn scenario_command_retry_after_timeout() -> Result<(), String> {
    const NAME: &str = "command_retry_after_timeout";
    let workspace = scenario_workspace(NAME)?;
    let mut plans = HashMap::new();
    plans.insert(
        "flaky".to_string(),
        VecDeque::from([
            MockCommandStep::DelaySuccess {
                delay_ms: 50,
                stdout: "late",
                stderr: "",
                exit_code: 0,
            },
            MockCommandStep::Success {
                stdout: "recovered",
                stderr: "",
                exit_code: 0,
            },
        ]),
    );
    let summary = run_yaml_scenario(
        NAME,
        &workspace,
        SCENARIO_COMMAND_RETRY_AFTER_TIMEOUT_YAML,
        command_deps(plans),
        None,
    )
    .await?;
    if task_output(&summary, NAME, "run")?["stdout"] != "recovered" {
        return Err(scenario_err(
            NAME,
            "expected second retry attempt to produce recovered output",
        ));
    }
    Ok(())
}

async fn scenario_read_control_from_trigger_path() -> Result<(), String> {
    const NAME: &str = "read_control_from_trigger_path";
    let workspace = tempdir().map_err(|err| scenario_err(NAME, err.to_string()))?;
    let control_file = workspace.path().join("control.json");
    fs::write(&control_file, r#"{"done": true, "message": "ok"}"#)
        .map_err(|err| scenario_err(NAME, err.to_string()))?;
    let summary = execute_yaml(
        workspace.path(),
        r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: read
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: read
      operator: ReadControlFileOperator
      params:
        path:
          $expr: "triggers.control_file"
      transitions:
        - to: done
          when:
            $expr: "tasks.read.output.done == true"
        - to: fail
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
    - id: fail
      operator: NoOpOperator
      terminal: failure
      params: {}
"#,
        BuiltinOperatorDeps::default(),
        Some(json!({ "control_file": control_file.display().to_string() })),
    )
    .await
    .map_err(|err| scenario_err(NAME, err.to_string()))?;
    let read = summary
        .completed_tasks
        .get("read")
        .ok_or_else(|| scenario_err(NAME, "missing read task result"))?;
    if read.output["done"] != true {
        return Err(scenario_err(NAME, "expected control-file done=true"));
    }
    Ok(())
}

async fn scenario_assert_completed_pass() -> Result<(), String> {
    const NAME: &str = "assert_completed_pass";
    let workspace = tempdir().map_err(|err| scenario_err(NAME, err.to_string()))?;
    let summary = execute_yaml(
        workspace.path(),
        r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: prep
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: prep
      operator: NoOpOperator
      params: {}
      transitions:
        - to: verify
    - id: verify
      operator: AssertCompletedOperator
      params:
        require: ["prep"]
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
"#,
        BuiltinOperatorDeps::default(),
        None,
    )
    .await
    .map_err(|err| scenario_err(NAME, err.to_string()))?;
    if !summary.completed_tasks.contains_key("verify") {
        return Err(scenario_err(NAME, "assert-completed task did not run"));
    }
    Ok(())
}

async fn scenario_assert_completed_missing_dependency_fails() -> Result<(), String> {
    const NAME: &str = "assert_completed_missing_dependency_fails";
    let workspace = tempdir().map_err(|err| scenario_err(NAME, err.to_string()))?;
    let err = execute_yaml(
        workspace.path(),
        r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: verify
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: verify
      operator: AssertCompletedOperator
      params:
        require: ["ghost"]
"#,
        BuiltinOperatorDeps::default(),
        None,
    )
    .await
    .expect_err("scenario should fail");
    if err.code != "WFG-EXEC-001" {
        return Err(scenario_err(
            NAME,
            format!("expected WFG-EXEC-001, got {}", err.code),
        ));
    }
    Ok(())
}

async fn scenario_human_approval_and_decision_path() -> Result<(), String> {
    const NAME: &str = "human_approval_and_decision_path";
    let workspace = scenario_workspace(NAME)?;
    let deps = BuiltinOperatorDeps {
        interviewer: Some(Arc::new(FakeInterviewer::approve_and_choose("ship"))),
        command_runner: None,
        engine_registry: None,
    };
    let summary = run_yaml_scenario(
        NAME,
        &workspace,
        SCENARIO_HUMAN_APPROVAL_AND_DECISION_PATH_YAML,
        deps,
        None,
    )
    .await?;
    expect_task_present(&summary, NAME, "done")?;
    if task_output(&summary, NAME, "approval")?["approved"] != true {
        return Err(scenario_err(NAME, "approval output should be true"));
    }
    if task_output(&summary, NAME, "decision")?["choice"] != "ship" {
        return Err(scenario_err(NAME, "decision output should be ship"));
    }
    let audit_path = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(summary.execution_id.to_string())
        .join("audit.jsonl");
    let audit_contents =
        fs::read_to_string(&audit_path).map_err(|err| scenario_err(NAME, err.to_string()))?;
    if audit_contents.lines().count() != 2 {
        return Err(scenario_err(
            NAME,
            format!(
                "expected 2 audit entries, got {}",
                audit_contents.lines().count()
            ),
        ));
    }
    Ok(())
}

async fn scenario_priority_branching() -> Result<(), String> {
    const NAME: &str = "priority_branching";
    let workspace = scenario_workspace(NAME)?;
    let summary = run_yaml_scenario(
        NAME,
        &workspace,
        SCENARIO_PRIORITY_BRANCHING_YAML,
        BuiltinOperatorDeps::default(),
        None,
    )
    .await?;
    expect_task_present(&summary, NAME, "high")?;
    expect_task_absent(&summary, NAME, "low")
}

async fn scenario_command_execution_error_fails_workflow() -> Result<(), String> {
    const NAME: &str = "command_execution_error_fails_workflow";
    let workspace = tempdir().map_err(|err| scenario_err(NAME, err.to_string()))?;
    let mut plans = HashMap::new();
    plans.insert(
        "fail_cmd".to_string(),
        VecDeque::from([MockCommandStep::Error {
            code: "WFG-CMD-MOCK",
            message: "mock failure",
        }]),
    );
    let deps = BuiltinOperatorDeps {
        interviewer: None,
        command_runner: Some(Arc::new(MockCommandRunner::new(plans))),
        engine_registry: None,
    };
    let err = execute_yaml(
        workspace.path(),
        r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: run
    max_time_seconds: 30
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 5
    max_workflow_iterations: 10
  tasks:
    - id: run
      operator: CommandOperator
      params:
        cmd: "fail_cmd"
"#,
        deps,
        None,
    )
    .await
    .expect_err("scenario should fail");
    if err.code != "WFG-EXEC-001" {
        return Err(scenario_err(
            NAME,
            format!("expected WFG-EXEC-001, got {}", err.code),
        ));
    }
    Ok(())
}
