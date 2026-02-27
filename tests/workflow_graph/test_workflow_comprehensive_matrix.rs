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
                .and_then(|queue| queue.pop_front())
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
        "basic_single_noop_success" => {
            let workspace = tempdir().map_err(|err| scenario_err(name, err.to_string()))?;
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
            .map_err(|err| scenario_err(name, err.to_string()))?;
            if !summary.completed_tasks.contains_key("start") {
                return Err(scenario_err(name, "expected start task to complete"));
            }
            Ok(())
        }
        "set_context_and_expression_branch" => {
            let workspace = tempdir().map_err(|err| scenario_err(name, err.to_string()))?;
            let summary = execute_yaml(
                workspace.path(),
                r#"
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
"#,
                BuiltinOperatorDeps::default(),
                None,
            )
            .await
            .map_err(|err| scenario_err(name, err.to_string()))?;
            if !summary.completed_tasks.contains_key("success") {
                return Err(scenario_err(name, "expected success branch to execute"));
            }
            if summary.completed_tasks.contains_key("failure") {
                return Err(scenario_err(name, "failure branch should not execute"));
            }
            Ok(())
        }
        "command_success_path" => {
            let workspace = tempdir().map_err(|err| scenario_err(name, err.to_string()))?;
            let mut plans = HashMap::new();
            plans.insert(
                "ok_cmd".to_string(),
                VecDeque::from([MockCommandStep::Success {
                    stdout: "hello",
                    stderr: "",
                    exit_code: 0,
                }]),
            );
            let deps = BuiltinOperatorDeps {
                interviewer: None,
                command_runner: Some(Arc::new(MockCommandRunner::new(plans))),
                engine_registry: None,
            };
            let summary = execute_yaml(
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
        cmd: "ok_cmd"
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
"#,
                deps,
                None,
            )
            .await
            .map_err(|err| scenario_err(name, err.to_string()))?;
            let run = summary
                .completed_tasks
                .get("run")
                .ok_or_else(|| scenario_err(name, "missing run task result"))?;
            if run.output["stdout"] != "hello" {
                return Err(scenario_err(name, "unexpected command stdout"));
            }
            Ok(())
        }
        "command_retry_after_timeout" => {
            let workspace = tempdir().map_err(|err| scenario_err(name, err.to_string()))?;
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
            let deps = BuiltinOperatorDeps {
                interviewer: None,
                command_runner: Some(Arc::new(MockCommandRunner::new(plans))),
                engine_registry: None,
            };
            let summary = execute_yaml(
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
"#,
                deps,
                None,
            )
            .await
            .map_err(|err| scenario_err(name, err.to_string()))?;
            let run = summary
                .completed_tasks
                .get("run")
                .ok_or_else(|| scenario_err(name, "missing run task result"))?;
            if run.output["stdout"] != "recovered" {
                return Err(scenario_err(
                    name,
                    "expected second retry attempt to produce recovered output",
                ));
            }
            Ok(())
        }
        "read_control_from_trigger_path" => {
            let workspace = tempdir().map_err(|err| scenario_err(name, err.to_string()))?;
            let control_file = workspace.path().join("control.json");
            fs::write(&control_file, r#"{"done": true, "message": "ok"}"#)
                .map_err(|err| scenario_err(name, err.to_string()))?;
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
            .map_err(|err| scenario_err(name, err.to_string()))?;
            let read = summary
                .completed_tasks
                .get("read")
                .ok_or_else(|| scenario_err(name, "missing read task result"))?;
            if read.output["done"] != true {
                return Err(scenario_err(name, "expected control-file done=true"));
            }
            Ok(())
        }
        "assert_completed_pass" => {
            let workspace = tempdir().map_err(|err| scenario_err(name, err.to_string()))?;
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
            .map_err(|err| scenario_err(name, err.to_string()))?;
            if !summary.completed_tasks.contains_key("verify") {
                return Err(scenario_err(name, "assert-completed task did not run"));
            }
            Ok(())
        }
        "assert_completed_missing_dependency_fails" => {
            let workspace = tempdir().map_err(|err| scenario_err(name, err.to_string()))?;
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
                    name,
                    format!("expected WFG-EXEC-001, got {}", err.code),
                ));
            }
            Ok(())
        }
        "human_approval_and_decision_path" => {
            let workspace = tempdir().map_err(|err| scenario_err(name, err.to_string()))?;
            let deps = BuiltinOperatorDeps {
                interviewer: Some(Arc::new(FakeInterviewer::approve_and_choose("ship"))),
                command_runner: None,
                engine_registry: None,
            };
            let summary = execute_yaml(
                workspace.path(),
                r#"
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
"#,
                deps,
                None,
            )
            .await
            .map_err(|err| scenario_err(name, err.to_string()))?;

            if !summary.completed_tasks.contains_key("done") {
                return Err(scenario_err(name, "workflow did not reach done"));
            }
            let approval = summary
                .completed_tasks
                .get("approval")
                .ok_or_else(|| scenario_err(name, "missing approval output"))?;
            if approval.output["approved"] != true {
                return Err(scenario_err(name, "approval output should be true"));
            }
            let decision = summary
                .completed_tasks
                .get("decision")
                .ok_or_else(|| scenario_err(name, "missing decision output"))?;
            if decision.output["choice"] != "ship" {
                return Err(scenario_err(name, "decision output should be ship"));
            }
            let audit_path = workspace
                .path()
                .join(".newton")
                .join("state")
                .join("workflows")
                .join(summary.execution_id.to_string())
                .join("audit.jsonl");
            let audit_contents = fs::read_to_string(&audit_path)
                .map_err(|err| scenario_err(name, err.to_string()))?;
            if audit_contents.lines().count() != 2 {
                return Err(scenario_err(
                    name,
                    format!(
                        "expected 2 audit entries, got {}",
                        audit_contents.lines().count()
                    ),
                ));
            }
            Ok(())
        }
        "priority_branching" => {
            let workspace = tempdir().map_err(|err| scenario_err(name, err.to_string()))?;
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
"#,
                BuiltinOperatorDeps::default(),
                None,
            )
            .await
            .map_err(|err| scenario_err(name, err.to_string()))?;
            if !summary.completed_tasks.contains_key("high") {
                return Err(scenario_err(name, "expected high priority target to run"));
            }
            if summary.completed_tasks.contains_key("low") {
                return Err(scenario_err(name, "low priority target should not run"));
            }
            Ok(())
        }
        "command_execution_error_fails_workflow" => {
            let workspace = tempdir().map_err(|err| scenario_err(name, err.to_string()))?;
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
                    name,
                    format!("expected WFG-EXEC-001, got {}", err.code),
                ));
            }
            Ok(())
        }
        _ => Err(format!("unknown scenario {name}")),
    }
}
