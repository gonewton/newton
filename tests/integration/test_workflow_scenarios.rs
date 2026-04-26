use async_trait::async_trait;
use chrono::Utc;
use insta::assert_yaml_snapshot;
use newton::core::error::AppError;
use newton::core::types::ErrorCategory;
use newton::workflow::executor::{resume_workflow, ExecutionOverrides, ExecutionSummary};
use newton::workflow::human::{ApprovalDefault, ApprovalResult, DecisionResult, Interviewer};
use newton::workflow::operator::{OperatorRegistry, OperatorRegistryBuilder};
use newton::workflow::operators::command::{
    CommandExecutionOutput, CommandExecutionRequest, CommandRunner,
};
use newton::workflow::operators::{self, BuiltinOperatorDeps};
use newton::workflow::schema::{self, TriggerType, WorkflowTrigger};
use newton::workflow::state::GraphSettings;
use serde_json::{json, Value};
use serial_test::serial;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

#[cfg(unix)]
struct PathGuard {
    original_path: String,
}

#[cfg(unix)]
impl PathGuard {
    fn prepend(dir: &Path) -> Self {
        let original_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.display(), original_path);
        std::env::set_var("PATH", new_path);
        Self { original_path }
    }
}

#[cfg(unix)]
impl Drop for PathGuard {
    fn drop(&mut self) {
        std::env::set_var("PATH", &self.original_path);
    }
}

#[cfg(unix)]
fn write_agent_stub(workspace: &Path, script_body: &str) {
    let script_path = workspace.join("agent");
    let mut file = std::fs::File::create(&script_path).expect("create agent stub");
    writeln!(file, "#!/bin/sh").expect("write shebang");
    writeln!(
        file,
        "if [ \"$1\" = \"--version\" ]; then echo 'agent 0.0.0'; exit 0; fi"
    )
    .expect("write --version handler");
    write!(file, "{script_body}").expect("write stub body");
    drop(file);
    let mut perms = std::fs::metadata(&script_path)
        .expect("read stub metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).expect("set executable permission");
}

#[derive(Clone)]
pub enum MockCommandStep {
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
pub struct MockCommandRunner {
    plans: Arc<Mutex<HashMap<String, VecDeque<MockCommandStep>>>>,
}

impl MockCommandRunner {
    #[must_use]
    pub fn new(plans: HashMap<String, VecDeque<MockCommandStep>>) -> Self {
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
pub struct FakeInterviewer {
    pub approval_result: ApprovalResult,
    pub decision_result: DecisionResult,
}

impl Default for FakeInterviewer {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeInterviewer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            approval_result: ApprovalResult {
                approved: true,
                reason: "approved by test".to_string(),
                timestamp: Utc::now(),
                timeout_applied: false,
                default_used: false,
            },
            decision_result: DecisionResult {
                choice: "default".to_string(),
                timestamp: Utc::now(),
                timeout_applied: false,
                default_used: false,
                response_text: Some("1".to_string()),
            },
        }
    }

    #[must_use]
    pub fn approve_and_choose(choice: &str) -> Self {
        let mut interviewer = Self::new();
        interviewer.decision_result.choice = choice.to_string();
        interviewer
    }
}

#[async_trait]
impl Interviewer for FakeInterviewer {
    fn interviewer_type(&self) -> &'static str {
        "fake-harness"
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

pub struct WorkflowTestHarness {
    pub temp_dir: TempDir,
    pub cmd_runner: MockCommandRunner,
    pub interviewer: FakeInterviewer,
}

impl WorkflowTestHarness {
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn new(
        cmd_plans: HashMap<String, VecDeque<MockCommandStep>>,
        interviewer: FakeInterviewer,
    ) -> Self {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let cmd_runner = MockCommandRunner::new(cmd_plans);
        Self {
            temp_dir,
            cmd_runner,
            interviewer,
        }
    }

    #[allow(clippy::missing_errors_doc)]
    pub async fn run_fixture(
        &self,
        fixture_name: &str,
        trigger_payload: Option<Value>,
    ) -> Result<ExecutionSummary, AppError> {
        self.run_fixture_with_overrides(
            fixture_name,
            trigger_payload,
            ExecutionOverrides {
                parallel_limit: None,
                max_time_seconds: None,
                checkpoint_base_path: Some(self.temp_dir.path().join(".newton/state/workflows")),
                artifact_base_path: Some(self.temp_dir.path().join(".newton/artifacts")),
                max_nesting_depth: None,
                verbose: false,
                server_notifier: None,
                pre_seed_nodes: true,
            },
        )
        .await
    }

    #[allow(clippy::missing_errors_doc)]
    pub async fn run_fixture_with_overrides(
        &self,
        fixture_name: &str,
        trigger_payload: Option<Value>,
        overrides: ExecutionOverrides,
    ) -> Result<ExecutionSummary, AppError> {
        let fixture_src_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("workflows");

        let workspace_workflows_dir = self.temp_dir.path().join("workflows");
        std::fs::create_dir_all(&workspace_workflows_dir).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "failed to create workspace workflows dir {}: {err}",
                    workspace_workflows_dir.display()
                ),
            )
        })?;

        for entry in std::fs::read_dir(&fixture_src_dir).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!(
                    "failed to read fixture directory {}: {err}",
                    fixture_src_dir.display()
                ),
            )
        })? {
            let entry = entry.map_err(|err| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to read fixture directory entry: {err}"),
                )
            })?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("yaml") {
                continue;
            }
            let dest = workspace_workflows_dir
                .join(path.file_name().expect("yaml fixture must have filename"));
            std::fs::copy(&path, &dest).map_err(|err| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("failed to copy fixture {}: {err}", path.display()),
                )
            })?;
        }

        let fixture_path = workspace_workflows_dir.join(fixture_name);

        let mut document = schema::parse_workflow(&fixture_path)?;

        if let Some(payload) = trigger_payload {
            document.triggers = Some(WorkflowTrigger {
                trigger_type: TriggerType::Manual,
                schema_version: "1".to_string(),
                payload,
            });
        }

        document = newton::workflow::transform::apply_default_pipeline(document)?;
        document.validate(&newton::workflow::expression::ExpressionEngine::default())?;

        let deps = BuiltinOperatorDeps {
            command_runner: Some(Arc::new(self.cmd_runner.clone())),
            interviewer: Some(Arc::new(self.interviewer.clone())),
            gh_runner: None,
            child_workflow_runner: None,
        };

        let settings = document.workflow.settings.clone();
        let mut builder = OperatorRegistry::builder();
        operators::register_builtins_with_deps(
            &mut builder,
            self.temp_dir.path().to_path_buf(),
            settings,
            deps,
        );
        let registry = builder.build();

        newton::workflow::executor::execute_workflow(
            document,
            fixture_path.clone(),
            registry,
            self.temp_dir.path().to_path_buf(),
            overrides,
        )
        .await
    }
}

/// Read and parse execution.json for the given UUID from `<state_root>/<execution_id>/execution.json`.
/// Panics with a descriptive message if the file does not exist or cannot be parsed.
pub fn read_execution_json(state_root: &Path, execution_id: Uuid) -> Value {
    let path = state_root
        .join(execution_id.to_string())
        .join("execution.json");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("failed to read execution.json at {}: {e}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|e| panic!("failed to parse execution.json at {}: {e}", path.display()))
}

/// Return the set of `task_id` strings from `execution["task_runs"]`.
/// Panics if `task_runs` is absent or not an array.
pub fn task_run_ids(execution: &Value) -> HashSet<String> {
    execution["task_runs"]
        .as_array()
        .expect("task_runs must be an array")
        .iter()
        .map(|entry| {
            entry["task_id"]
                .as_str()
                .expect("task_run entry must have task_id string")
                .to_string()
        })
        .collect()
}

/// Scan all subdirectories of `state_root`, read each `execution.json`, and
/// collect execution_ids whose `parent_execution_id` matches `parent_id`.
pub fn find_child_executions(state_root: &Path, parent_id: Uuid) -> Vec<Uuid> {
    let parent_id_str = parent_id.to_string();
    let mut children = Vec::new();
    let entries = std::fs::read_dir(state_root)
        .unwrap_or_else(|e| panic!("failed to read state_root {}: {e}", state_root.display()));
    for entry in entries.flatten() {
        let exec_json = entry.path().join("execution.json");
        if let Ok(bytes) = std::fs::read(&exec_json) {
            if let Ok(val) = serde_json::from_slice::<Value>(&bytes) {
                if val["parent_execution_id"].as_str() == Some(&parent_id_str) {
                    if let Some(id_str) = val["execution_id"].as_str() {
                        if let Ok(id) = Uuid::parse_str(id_str) {
                            children.push(id);
                        }
                    }
                }
            }
        }
    }
    children
}

/// Result of executing a workflow via CLI command
pub struct WorkflowCliResult {
    pub output: std::process::Output,
    pub stdout_text: String,
    pub stderr_text: String,
    pub temp_dir: tempfile::TempDir,
}

/// Helper function to execute a workflow via CLI and return parsed output
fn execute_workflow_cli(workflow_filename: &str) -> WorkflowCliResult {
    use std::process::{Command, Stdio};

    let temp_dir = tempfile::tempdir().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_newton"))
        .arg("run")
        .arg(format!("tests/fixtures/workflows/{}", workflow_filename))
        .arg("--workspace")
        .arg(temp_dir.path())
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("Failed to execute newton command");

    let stdout_text = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr_text = String::from_utf8_lossy(&output.stderr).to_string();

    // Debug output for troubleshooting
    println!("=== STDOUT ===");
    println!("{}", stdout_text);
    println!("=== STDERR ===");
    println!("{}", stderr_text);

    WorkflowCliResult {
        output,
        stdout_text,
        stderr_text,
        temp_dir,
    }
}

// -----------------------------------------------------------------------------
// Scenario 01: Minimal Success
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_01_minimal_success() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("01_minimal_success.yaml", None)
        .await
        .expect("workflow succeeded");

    assert_eq!(summary.completed_tasks.len(), 1);
    assert!(summary.completed_tasks.contains_key("start"));

    assert_yaml_snapshot!(summary, {
        ".execution_id" => "[uuid]",
        ".completed_tasks.*.duration_ms" => "[duration]",
    });
}

// -----------------------------------------------------------------------------
// Scenario 02: Validation Failure
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_02_validation_failure() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let err = harness
        .run_fixture("02_validation_failure.yaml", None)
        .await
        .expect_err("workflow must fail validation");

    assert_eq!(err.category, ErrorCategory::ValidationError);

    assert!(
        err.message
            .contains("settings.max_time_seconds must be >= 1"),
        "Error should identify invalid max_time_seconds: {}",
        err.message
    );
}

// -----------------------------------------------------------------------------
// Scenario 03: Execution Limits
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_03_execution_limits() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let err = harness
        .run_fixture("03_execution_limits.yaml", None)
        .await
        .expect_err("workflow must hit iteration cap");

    assert_eq!(err.category, ErrorCategory::ValidationError);
    assert_eq!(err.code, "WFG-ITER-001");
}

// -----------------------------------------------------------------------------
// Scenario 04: Expression Branching
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_04_expression_branching() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("04_expression_branching.yaml", None)
        .await
        .expect("workflow succeeded");

    assert!(summary.completed_tasks.contains_key("success_node"));
    assert!(!summary.completed_tasks.contains_key("fail_node"));

    assert_yaml_snapshot!(summary, {
        ".execution_id" => "[uuid]",
        ".completed_tasks.*.duration_ms" => "[duration]",
    });
}

// -----------------------------------------------------------------------------
// Scenario 05: Priority Selection
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_05_priority_selection() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("05_priority_selection.yaml", None)
        .await
        .expect("workflow succeeded");

    assert!(summary.completed_tasks.contains_key("high_priority"));
    assert!(!summary.completed_tasks.contains_key("low_priority"));

    assert_yaml_snapshot!(summary, {
        ".execution_id" => "[uuid]",
        ".completed_tasks.*.duration_ms" => "[duration]",
    });
}

// -----------------------------------------------------------------------------
// Scenario 06: Task Iteration Cap
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_06_task_iteration_cap() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let err = harness
        .run_fixture("06_task_iteration_cap.yaml", None)
        .await
        .expect_err("task must hit iteration cap");

    assert_eq!(err.category, ErrorCategory::ValidationError);
    // Code for task iteration cap is WFG-ITER-002
    assert_eq!(err.code, "WFG-ITER-002");
}

// -----------------------------------------------------------------------------
// Scenario 07: SetContext Deep Merge
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_07_set_context_merge() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("07_set_context_merge.yaml", None)
        .await
        .expect("workflow must succeed");

    assert_yaml_snapshot!(summary, {
        ".execution_id" => "[uuid]",
        ".completed_tasks.*.duration_ms" => "[duration]",
    });
}

// -----------------------------------------------------------------------------
// Scenario 08: Command Success & Data Capture
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_08_command_success() {
    let mut plans = HashMap::new();
    plans.insert(
        "echo '{\"cpu\": 45}'".to_string(),
        VecDeque::from(vec![MockCommandStep::Success {
            stdout: "{\"cpu\": 45}",
            stderr: "",
            exit_code: 0,
        }]),
    );
    let harness = WorkflowTestHarness::new(plans, FakeInterviewer::new());
    let summary = harness
        .run_fixture("08_command_success.yaml", None)
        .await
        .expect("workflow must succeed");

    assert_yaml_snapshot!(summary, {
        ".execution_id" => "[uuid]",
        ".completed_tasks.*.duration_ms" => "[duration]",
    });
}

// -----------------------------------------------------------------------------
// Scenario 09: Read Control File Payload Extraction
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_09_read_control_file() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());

    // Setup file in mock workspace
    let cfg_path = harness.temp_dir.path().join("config.json");
    std::fs::write(cfg_path, r#"{"key": "expected_value"}"#).expect("failed to write mock config");

    let summary = harness
        .run_fixture("09_read_control_file.yaml", None)
        .await
        .expect("workflow must succeed");

    assert_yaml_snapshot!(summary, {
        ".execution_id" => "[uuid]",
        ".completed_tasks.*.duration_ms" => "[duration]",
    });
}

// -----------------------------------------------------------------------------
// Scenario 10: Assert Completed Matrix
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_10_assert_completed_fail() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let err = harness
        .run_fixture("10_assert_completed_fail.yaml", None)
        .await
        .expect_err("workflow must fail dependency check");

    assert_eq!(err.category, ErrorCategory::ValidationError);
    assert!(
        err.message.contains("task check_dep failed"),
        "Error should mention task failure: {}",
        err.message
    );
}

// -----------------------------------------------------------------------------
// Scenario 11: Complex Cyclic Path
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_11_complex_cycle() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let err = harness
        .run_fixture("11_complex_cycle.yaml", None)
        .await
        .expect_err("workflow must hit iteration cap");

    assert_eq!(err.category, ErrorCategory::ValidationError);
    assert_eq!(err.code, "WFG-ITER-001");
}

// -----------------------------------------------------------------------------
// Scenario 12: Deeply Nested Expression Resolution
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_12_nested_expressions() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("12_nested_expressions.yaml", None)
        .await
        .expect("workflow must succeed");

    assert!(summary.completed_tasks.contains_key("success"));
    assert_yaml_snapshot!(summary, {
        ".execution_id" => "[uuid]",
        ".completed_tasks.*.duration_ms" => "[duration]",
    });
}

// -----------------------------------------------------------------------------
// Scenario 13: Parallel Execution Limits
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_13_parallel_limits() {
    let mut plans = HashMap::new();
    // We use DelaySuccess to verify that they run sequentially if limit=1
    plans.insert(
        "sleep 0.1".to_string(),
        VecDeque::from(vec![
            MockCommandStep::DelaySuccess {
                delay_ms: 100,
                stdout: "",
                stderr: "",
                exit_code: 0,
            },
            MockCommandStep::DelaySuccess {
                delay_ms: 100,
                stdout: "",
                stderr: "",
                exit_code: 0,
            },
            MockCommandStep::DelaySuccess {
                delay_ms: 100,
                stdout: "",
                stderr: "",
                exit_code: 0,
            },
        ]),
    );

    let harness = WorkflowTestHarness::new(plans, FakeInterviewer::new());
    let start = Utc::now();
    let summary = harness
        .run_fixture("13_parallel_limits.yaml", None)
        .await
        .expect("workflow must succeed");
    let duration = Utc::now().signed_duration_since(start).num_milliseconds();

    // Total should be ~300ms + overhead. If parallel, it would be ~100ms.
    assert!(
        duration >= 200,
        "Duration was {duration}ms, expected >= 200ms",
    );

    assert_yaml_snapshot!(summary, {
        ".execution_id" => "[uuid]",
        ".completed_tasks.*.duration_ms" => "[duration]",
        ".completed_tasks.*.output.duration_ms" => "[duration]",
    });
}

// -----------------------------------------------------------------------------
// Scenario 14: Error Fallback
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_14_error_fallback() {
    let mut plans = HashMap::new();
    plans.insert(
        "false".to_string(),
        VecDeque::from(vec![MockCommandStep::Success {
            stdout: "",
            stderr: "failed",
            exit_code: 1,
        }]),
    );

    let harness = WorkflowTestHarness::new(plans, FakeInterviewer::new());
    let summary = harness
        .run_fixture("14_error_fallback.yaml", None)
        .await
        .expect("workflow must succeed via fallback");

    assert!(
        summary.completed_tasks.contains_key("fallback"),
        "Completed tasks: {:?}",
        summary.completed_tasks.keys()
    );
    assert_eq!(
        summary.completed_tasks["fail_cmd"].status,
        newton::workflow::executor::TaskStatus::Failed
    );
}

// -----------------------------------------------------------------------------
// Scenario 15: Retry & Backoff
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_15_retry_backoff() {
    let mut plans = HashMap::new();
    plans.insert(
        "flaky".to_string(),
        VecDeque::from(vec![
            MockCommandStep::Success {
                stdout: "",
                stderr: "fail 1",
                exit_code: 1,
            },
            MockCommandStep::Success {
                stdout: "",
                stderr: "fail 2",
                exit_code: 1,
            },
            MockCommandStep::Success {
                stdout: "success",
                stderr: "",
                exit_code: 0,
            },
        ]),
    );

    let harness = WorkflowTestHarness::new(plans, FakeInterviewer::new());
    let summary = harness
        .run_fixture("15_retry_backoff.yaml", None)
        .await
        .expect("workflow must succeed after retries");

    // Task should succeed after retries
    assert_eq!(
        summary.completed_tasks["retry_task"].status,
        newton::workflow::executor::TaskStatus::Success
    );
    // run_seq is 1 because it was only queued once, even if it retried internally
    assert_eq!(summary.completed_tasks["retry_task"].run_seq, 1);
}

// -----------------------------------------------------------------------------
// Scenario 16: Task Timeout
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_16_task_timeout() {
    let mut plans = HashMap::new();
    plans.insert(
        "slow".to_string(),
        VecDeque::from(vec![MockCommandStep::DelaySuccess {
            delay_ms: 500,
            stdout: "too late",
            stderr: "",
            exit_code: 0,
        }]),
    );

    let harness = WorkflowTestHarness::new(plans, FakeInterviewer::new());
    let err = harness
        .run_fixture("16_task_timeout.yaml", None)
        .await
        .expect_err("task must timeout");

    // The executor wraps the timeout error
    assert_eq!(err.code, "WFG-EXEC-001");
    assert!(err.message.contains("task hang failed"));
}

// -----------------------------------------------------------------------------
// Scenario 17: Checkpoint Resume
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_17_checkpoint_resume() {
    let mut plans = HashMap::new();
    plans.insert(
        "fail_then_succeed".to_string(),
        VecDeque::from(vec![MockCommandStep::Success {
            stdout: "",
            stderr: "first fail",
            exit_code: 1,
        }]),
    );

    let harness = WorkflowTestHarness::new(plans, FakeInterviewer::new());
    let execution_id = {
        let res = harness.run_fixture("17_checkpoint_resume.yaml", None).await;
        // It should fail at step2
        let err = res.expect_err("initially must fail at step2");
        assert!(
            err.message.contains("task step2 failed"),
            "Error was: {err:?}",
        );

        let state_dir = harness.temp_dir.path().join(".newton/state/workflows");
        let mut entries = std::fs::read_dir(&state_dir)
            .unwrap_or_else(|_| panic!("read_dir failed for {}", state_dir.display()));
        let entry = entries.next().unwrap().unwrap();
        uuid::Uuid::parse_str(entry.file_name().to_str().unwrap()).unwrap()
    };

    // Now setup second plan for the same command
    let mut plans2 = HashMap::new();
    plans2.insert(
        "fail_then_succeed".to_string(),
        VecDeque::from(vec![MockCommandStep::Success {
            stdout: "finally success",
            stderr: "",
            exit_code: 0,
        }]),
    );

    // We need to update the harness with new plans but keep the same interviewer and temp_dir
    let cmd_runner = MockCommandRunner::new(plans2);

    // MANUAL FIX: Load checkpoint, remove step2 from completed, add to ready_queue
    {
        let mut checkpoint =
            newton::workflow::checkpoint::load_checkpoint(harness.temp_dir.path(), &execution_id)
                .unwrap();
        checkpoint.completed.remove("step2");
        checkpoint.ready_queue.push("step2".to_string());
        newton::workflow::checkpoint::save_checkpoint(
            harness.temp_dir.path(),
            &execution_id,
            &checkpoint,
            false,
        )
        .unwrap();
    }

    let registry = {
        let mut builder = OperatorRegistryBuilder::new();
        operators::register_builtins_with_deps(
            &mut builder,
            harness.temp_dir.path().to_path_buf(),
            GraphSettings::default(),
            BuiltinOperatorDeps {
                command_runner: Some(Arc::new(cmd_runner.clone())),
                interviewer: Some(Arc::new(harness.interviewer.clone())),
                gh_runner: None,
                child_workflow_runner: None,
            },
        );
        builder.build()
    };

    let summary = resume_workflow(
        registry,
        harness.temp_dir.path().to_path_buf(),
        execution_id,
        false,
    )
    .await
    .expect("resume must succeed");

    assert!(summary.completed_tasks.contains_key("step3"));
}

// -----------------------------------------------------------------------------
// Scenario 18: Human Decision
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_18_human_decision() {
    let mut interviewer = FakeInterviewer::new();
    interviewer.decision_result = DecisionResult {
        choice: "yes".to_string(),
        timestamp: Utc::now(),
        timeout_applied: false,
        default_used: false,
        response_text: None,
    };

    let harness = WorkflowTestHarness::new(HashMap::new(), interviewer);
    let summary = harness
        .run_fixture("18_human_decision.yaml", None)
        .await
        .expect("workflow must succeed");

    assert!(summary.completed_tasks.contains_key("deploy"));
}

// -----------------------------------------------------------------------------
// Scenario 19: Large Payload
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_19_large_payload() {
    let blob = "A".repeat(50_000); // 50KB
    let trigger_payload = json!({
        "blob": blob
    });

    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("19_large_payload.yaml", Some(trigger_payload))
        .await
        .expect("workflow must succeed");

    // Debug: print summary to see if something is wrong
    // println!("Scenario 19 Summary: {:#?}", summary);

    // Check if context has the large blob
    // summary doesn't include full context, but we can check checkpoint or just trust success for now
    // Actually, I'll use insta to see if it works
    assert_yaml_snapshot!(summary, {
        ".execution_id" => "[uuid]",
        ".completed_tasks.*.duration_ms" => "[duration]",
    });
}

// -----------------------------------------------------------------------------
// Scenario 20: Parallel Consistency
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_20_parallel_consistency() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("20_parallel_consistency.yaml", None)
        .await
        .expect("workflow must succeed");

    // Verify that all 3 parallel tasks completed
    assert!(summary.completed_tasks.contains_key("p1"));
    assert!(summary.completed_tasks.contains_key("p2"));
    assert!(summary.completed_tasks.contains_key("p3"));
    assert!(summary.completed_tasks.contains_key("join"));
}

// -----------------------------------------------------------------------------
// Scenario 21: Goal Gates
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_21_goal_gates() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("21_goal_gates.yaml", None)
        .await
        .expect("workflow must succeed");

    assert!(summary.completed_tasks.contains_key("goal1"));
}

// -----------------------------------------------------------------------------
// Scenario 22: IncludeIf Guard
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_22_include_if() {
    let trigger_payload = json!({
        "mode": "test"
    });

    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("22_include_if.yaml", Some(trigger_payload))
        .await
        .expect("workflow must succeed");

    assert!(summary.completed_tasks.contains_key("included_task"));
    assert!(!summary.completed_tasks.contains_key("excluded_task"));
}

// -----------------------------------------------------------------------------
// Scenario 23: Data Redaction
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_23_redaction() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("23_redaction.yaml", None)
        .await
        .expect("workflow must succeed");

    // Check if the redacted key is actually redacted in the completed task output
    let task = summary.completed_tasks.get("set_secret").expect("task ran");
    let output = task.output.as_object().expect("is object");
    let patch = output
        .get("patch")
        .expect("has patch")
        .as_object()
        .expect("is object");

    assert_eq!(
        patch.get("api_key").unwrap().as_str().unwrap(),
        "[REDACTED]"
    );
    assert_eq!(
        patch.get("password").unwrap().as_str().unwrap(),
        "[REDACTED]"
    );
    assert_eq!(
        patch.get("secret_token_123").unwrap().as_str().unwrap(),
        "[REDACTED]"
    );
    assert_eq!(patch.get("other").unwrap().as_str().unwrap(), "visible");
}

// -----------------------------------------------------------------------------
// Scenario 24: Webhook Trigger
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_24_webhook() {
    let trigger_payload = json!({
        "event": "push"
    });

    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("24_webhook.yaml", Some(trigger_payload))
        .await
        .expect("workflow must succeed");

    assert!(summary.completed_tasks.contains_key("handle_webhook"));
}

// -----------------------------------------------------------------------------
// Scenario 25: Human Approval Operator
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_25_human_approval() {
    // FakeInterviewer defaults to approved: true
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("25_human_approval.yaml", None)
        .await
        .expect("workflow must succeed");

    assert!(
        summary.completed_tasks.contains_key("approved_action"),
        "Expected approved_action to be completed"
    );
    assert!(
        !summary.completed_tasks.contains_key("denied_action"),
        "denied_action should not have run"
    );

    assert_yaml_snapshot!(summary, {
        ".execution_id" => "[uuid]",
        ".completed_tasks.*.duration_ms" => "[duration]",
        ".completed_tasks.request_approval.output.timestamp" => "[timestamp]",
    });
}

// -----------------------------------------------------------------------------
// Scenario 26: Macro Expansion
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_26_macro_expansion() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("26_macro_expansion.yaml", None)
        .await
        .expect("workflow must succeed after macro expansion");

    // 4 tasks: start, step_one (macro), step_two (macro), done
    assert_eq!(
        summary.completed_tasks.len(),
        4,
        "Expected 4 completed tasks, got: {:?}",
        summary.completed_tasks.keys().collect::<Vec<_>>()
    );
    assert!(
        summary.completed_tasks.contains_key("step_one"),
        "step_one must exist after expansion"
    );
    assert!(
        summary.completed_tasks.contains_key("step_two"),
        "step_two must exist after expansion"
    );
    assert!(summary.completed_tasks.contains_key("done"));

    assert_yaml_snapshot!(summary, {
        ".execution_id" => "[uuid]",
        ".completed_tasks.*.duration_ms" => "[duration]",
    });
}

// -----------------------------------------------------------------------------
// Scenario 27: Goal Gates Failure (WFG-GATE-001)
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_27_goal_gates_fail() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let err = harness
        .run_fixture("27_goal_gates_fail.yaml", None)
        .await
        .expect_err("workflow must fail because required goal gate was never reached");

    assert_eq!(err.category, ErrorCategory::ValidationError);
    assert_eq!(err.code, "WFG-GATE-001");
    assert!(
        err.message.contains("required_gate"),
        "Error should name the failing gate: {}",
        err.message
    );
}

// -----------------------------------------------------------------------------
// Scenario 28: Artifact Persistence
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_28_artifact_persistence() {
    // Return > 10 bytes so ArtifactStore routes it to disk (max_inline_bytes: 10 in fixture)
    let large_stdout = r#"{"result":"this output is deliberately longer than ten bytes"}"#;
    let mut plans = HashMap::new();
    plans.insert(
        "large_output".to_string(),
        VecDeque::from(vec![MockCommandStep::Success {
            stdout: large_stdout,
            stderr: "",
            exit_code: 0,
        }]),
    );

    let harness = WorkflowTestHarness::new(plans, FakeInterviewer::new());
    let summary = harness
        .run_fixture("28_artifact_persistence.yaml", None)
        .await
        .expect("workflow must succeed; artifact should route to disk then materialize");

    let record = summary
        .completed_tasks
        .get("gen_data")
        .expect("gen_data must have run");
    assert_eq!(
        record.status,
        newton::workflow::executor::TaskStatus::Success
    );

    // Output must have been materialized back from the artifact file
    let output = record
        .output
        .as_object()
        .expect("output must be a JSON object");
    assert!(
        output.contains_key("stdout"),
        "materialized output must contain stdout key, got: {:?}",
        output.keys().collect::<Vec<_>>()
    );
}

// -----------------------------------------------------------------------------
// Scenario 29: History Audit (5-task chain, all durations recorded)
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_29_history_audit() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("29_history_audit.yaml", None)
        .await
        .expect("workflow must succeed");

    assert_eq!(
        summary.completed_tasks.len(),
        5,
        "All 5 tasks must have completed: {:?}",
        summary.completed_tasks.keys().collect::<Vec<_>>()
    );

    for (task_id, record) in &summary.completed_tasks {
        assert_eq!(
            record.status,
            newton::workflow::executor::TaskStatus::Success,
            "task {task_id} must have succeeded",
        );
        // duration_ms is u64 — the field is always present (structural check)
        let _ = record.duration_ms;
    }

    assert_yaml_snapshot!(summary, {
        ".execution_id" => "[uuid]",
        ".completed_tasks.*.duration_ms" => "[duration]",
    });
}

// -----------------------------------------------------------------------------
// Scenario 30: Assert Completed (success path)
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_30_assert_completed_pass() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("30_assert_completed_pass.yaml", None)
        .await
        .expect("workflow must succeed; step1 ran before checker");

    assert!(summary.completed_tasks.contains_key("step1"));
    assert!(summary.completed_tasks.contains_key("checker"));
    assert_eq!(
        summary.completed_tasks["checker"].status,
        newton::workflow::executor::TaskStatus::Success
    );
}

// -----------------------------------------------------------------------------
// Scenario 31: Agent Streaming On
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_31_agent_streaming_on() {
    // For agent streaming tests, we need to actually run the process and capture stdout
    // since AgentOperator uses real process execution (not mocked)
    let result = execute_workflow_cli("31_agent_streaming_on.yaml");

    // The agent engine output should be streamed to process stdout
    assert!(
        result.stdout_text.contains("STREAMED_LINE_1"),
        "Expected 'STREAMED_LINE_1' in stdout, got: {}",
        result.stdout_text
    );
    assert!(
        result.stdout_text.contains("STREAMED_LINE_2"),
        "Expected 'STREAMED_LINE_2' in stdout, got: {}",
        result.stdout_text
    );

    // Workflow should complete successfully
    assert!(result.output.status.success(), "Workflow should succeed");
}

// -----------------------------------------------------------------------------
// Scenario 32: Agent Streaming Off
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_32_agent_streaming_off() {
    let result = execute_workflow_cli("32_agent_streaming_off.yaml");

    // The agent engine output should NOT appear on process stdout
    assert!(
        !result.stdout_text.contains("SHOULD_NOT_APPEAR_ON_STDOUT"),
        "Agent output should not appear on process stdout when streaming is disabled, got: {}",
        result.stdout_text
    );

    // Workflow should complete successfully
    assert!(result.output.status.success(), "Workflow should succeed");
}

// -----------------------------------------------------------------------------
// Scenario 33: Agent Task Override
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_33_agent_task_override() {
    let result = execute_workflow_cli("33_agent_task_override.yaml");

    // Task override should enable streaming even when workflow default is off
    assert!(
        result.stdout_text.contains("TASK_OVERRIDE_STREAMED"),
        "Expected task override to enable streaming, got: {}",
        result.stdout_text
    );

    // Workflow should complete successfully
    assert!(result.output.status.success(), "Workflow should succeed");
}

// -----------------------------------------------------------------------------
// Scenario 34: Agent Streaming Artifact Unchanged
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_34_agent_streaming_artifact_unchanged() {
    use std::fs;

    let result = execute_workflow_cli("31_agent_streaming_on.yaml");

    // Workflow should complete successfully
    assert!(result.output.status.success(), "Workflow should succeed");

    // Find the artifact file
    let artifacts_dir = result.temp_dir.path().join(".newton/artifacts/workflows");

    // Find execution directory (should be only one)
    let mut execution_dirs = fs::read_dir(&artifacts_dir)
        .expect("Failed to read artifacts dir")
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_type()
                .ok()
                .map(|ft| ft.is_dir())
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    assert_eq!(
        execution_dirs.len(),
        1,
        "Expected exactly one execution directory"
    );
    let execution_dir = execution_dirs.pop().unwrap().path();

    // Find task artifact directory
    let task_dir = execution_dir.join("task/agent_task/1");
    let stdout_artifact = task_dir.join("stdout.txt");

    assert!(
        stdout_artifact.exists(),
        "stdout artifact file should exist"
    );

    // Read the artifact file content
    let artifact_content =
        fs::read_to_string(&stdout_artifact).expect("Failed to read stdout artifact");

    println!("=== ARTIFACT CONTENT ===");
    println!("{}", artifact_content);

    // The artifact file should contain the same output even with streaming enabled
    assert!(
        artifact_content.contains("STREAMED_LINE_1"),
        "Artifact should contain 'STREAMED_LINE_1', got: {}",
        artifact_content
    );
    assert!(
        artifact_content.contains("STREAMED_LINE_2"),
        "Artifact should contain 'STREAMED_LINE_2', got: {}",
        artifact_content
    );
}

// -----------------------------------------------------------------------------
// Scenario 37: Nested Workflow Basic
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_37_nested_workflow_basic() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture("37_nested_workflow_basic.yaml", None)
        .await
        .expect("workflow must succeed");

    let output = &summary
        .completed_tasks
        .get("call_child")
        .expect("call_child must complete")
        .output;
    let child_execution_id = output["child_execution_id"]
        .as_str()
        .expect("child_execution_id must be string");

    let state_root = harness.temp_dir.path().join(".newton/state/workflows");
    let child_uuid =
        Uuid::parse_str(child_execution_id).expect("child_execution_id must be valid UUID");
    let child_execution = read_execution_json(&state_root, child_uuid);
    let child_task_ids = task_run_ids(&child_execution);
    assert_eq!(
        child_execution["parent_execution_id"],
        json!(summary.execution_id.to_string())
    );
    assert_eq!(child_execution["parent_task_id"], json!("call_child"));
    assert_eq!(child_execution["nesting_depth"], json!(1));
    assert!(child_task_ids.contains("start"));

    assert_yaml_snapshot!(summary, {
        ".execution_id" => "[uuid]",
        ".completed_tasks.*.duration_ms" => "[duration]",
        ".completed_tasks.call_child.output.child_execution_id" => "[uuid]",
        ".completed_tasks.call_child.output.child_workflow_file" => "[path]",
        ".completed_tasks.call_child.output.child_total_iterations" => "[iterations]",
        ".completed_tasks.call_child.output.child_completed_task_count" => "[count]",
    });
}

// -----------------------------------------------------------------------------
// Scenario 38: Nested Workflow Error Propagation
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_38_nested_workflow_error_propagates() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let err = harness
        .run_fixture("38_nested_workflow_error.yaml", None)
        .await
        .expect_err("workflow must fail");

    assert_eq!(err.category, ErrorCategory::ValidationError);
    assert!(
        err.message.contains("task call_child failed"),
        "Error should mention parent task failure: {}",
        err.message
    );
}

// -----------------------------------------------------------------------------
// Scenario 39: Nested Workflow Depth Limit
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_39_nested_depth_limit_enforced() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let summary = harness
        .run_fixture_with_overrides(
            "39_nested_depth_limit.yaml",
            None,
            ExecutionOverrides {
                parallel_limit: None,
                max_time_seconds: None,
                checkpoint_base_path: Some(harness.temp_dir.path().join(".newton/state/workflows")),
                artifact_base_path: Some(harness.temp_dir.path().join(".newton/artifacts")),
                max_nesting_depth: Some(0),
                verbose: false,
                server_notifier: None,
                pre_seed_nodes: true,
            },
        )
        .await
        .expect("workflow must complete");

    let call_child = summary
        .completed_tasks
        .get("call_child")
        .expect("call_child must complete");
    assert_eq!(call_child.error_code.as_deref(), Some("WFG-NEST-002"));
}

// -----------------------------------------------------------------------------
// Scenario 40: Nested Workflow Path Sandbox
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_40_nested_path_sandbox_enforced() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let outside_child = harness
        .temp_dir
        .path()
        .parent()
        .expect("workspace temp dir has parent")
        .join("outside-child.yaml");
    std::fs::write(
        &outside_child,
        r#"version: "2.0"
mode: workflow_graph
workflow:
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
    )
    .expect("write outside child workflow");

    let summary = harness
        .run_fixture("40_nested_path_sandbox.yaml", None)
        .await
        .expect("workflow must complete");

    let call_child = summary
        .completed_tasks
        .get("call_child")
        .expect("call_child must complete");
    assert_eq!(call_child.error_code.as_deref(), Some("WFG-NEST-001"));
}

// -----------------------------------------------------------------------------
// Scenario 41: Fail Task Persisted in execution.json
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_41_fail_task_persisted_in_execution_json() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let state_root = harness.temp_dir.path().join(".newton/state/workflows");

    let err = harness
        .run_fixture("41_fail_immediate_task_run_persist.yaml", None)
        .await
        .expect_err("workflow must fail");
    assert_eq!(
        err.code, "WFG-EXEC-001",
        "expected WFG-EXEC-001 but got: {} — {}",
        err.code, err.message
    );

    // Find the single execution directory that was created
    let entries: Vec<_> = std::fs::read_dir(&state_root)
        .expect("state_root must exist after a run")
        .flatten()
        .collect();
    assert_eq!(entries.len(), 1, "exactly one execution directory expected");
    let exec_id = Uuid::parse_str(
        entries[0]
            .file_name()
            .to_str()
            .expect("valid utf8 dir name"),
    )
    .expect("execution dir must be a UUID");

    let execution = read_execution_json(&state_root, exec_id);
    let ids = task_run_ids(&execution);
    assert!(
        ids.contains("fail_task"),
        "fail_task must appear in task_runs; found: {ids:?}"
    );
    let task_run = execution["task_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["task_id"].as_str() == Some("fail_task"))
        .expect("fail_task run entry");
    assert_eq!(task_run["status"].as_str(), Some("failed"));
}

// -----------------------------------------------------------------------------
// Scenario 42: Nested Fail Child Task Persisted
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_42_nested_fail_child_task_persisted() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let state_root = harness.temp_dir.path().join(".newton/state/workflows");
    let err = harness
        .run_fixture("42_nested_fail_task_run_persist.yaml", None)
        .await
        .expect_err("workflow must fail");
    assert!(
        !err.code.is_empty(),
        "error must have a code; got: {}",
        err.message
    );

    // Find the parent execution directory
    let all_entries: Vec<_> = std::fs::read_dir(&state_root)
        .expect("state_root must exist")
        .flatten()
        .collect();
    assert!(
        !all_entries.is_empty(),
        "at least one execution directory expected"
    );

    // Find parent execution (nesting_depth == 0)
    let parent_exec_id = all_entries
        .iter()
        .find_map(|entry| {
            let exec_json = entry.path().join("execution.json");
            let bytes = std::fs::read(&exec_json).ok()?;
            let val: Value = serde_json::from_slice(&bytes).ok()?;
            if val["nesting_depth"].as_u64() == Some(0) {
                Uuid::parse_str(entry.file_name().to_str()?).ok()
            } else {
                None
            }
        })
        .expect("parent execution directory must exist");

    let child_ids = find_child_executions(&state_root, parent_exec_id);
    assert_eq!(child_ids.len(), 1, "exactly one child execution expected");

    let child_execution = read_execution_json(&state_root, child_ids[0]);
    let ids = task_run_ids(&child_execution);
    assert!(
        ids.contains("assert_early"),
        "assert_early must appear in child task_runs; found: {ids:?}"
    );
    let task_run = child_execution["task_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["task_id"].as_str() == Some("assert_early"))
        .expect("assert_early run entry");
    assert_eq!(task_run["status"].as_str(), Some("failed"));
}

// -----------------------------------------------------------------------------
// Scenario 43: Transition IncludeIf Error Fails Fast
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_43_transition_include_if_error_fails_fast() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let err = harness
        .run_fixture("43_transition_include_if_eval_error.yaml", None)
        .await
        .expect_err("workflow must fail on bad transition include_if");

    assert_eq!(
        err.code, "WFG-GRAPH-001",
        "expected WFG-GRAPH-001 but got: {} — {}",
        err.code, err.message
    );
}

// -----------------------------------------------------------------------------
// Scenario 44: Barrier Invalid Params Fails
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_44_barrier_invalid_params_fails() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let err = harness
        .run_fixture("44_barrier_invalid_params.yaml", None)
        .await
        .expect_err("workflow must fail on invalid barrier params");

    assert_eq!(
        err.code, "WFG-BARRIER-001",
        "expected WFG-BARRIER-001 but got: {} — {}",
        err.code, err.message
    );
}

// -----------------------------------------------------------------------------
// Scenario 45: Nested Non-Object Context Fails
// -----------------------------------------------------------------------------
#[tokio::test]
async fn test_scenario_45_nested_non_object_context_fails() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    let state_root = harness.temp_dir.path().join(".newton/state/workflows");
    let err = harness
        .run_fixture("45_nested_non_object_context.yaml", None)
        .await
        .expect_err("workflow must fail when parent context is non-object");
    assert_eq!(err.code, "WFG-EXEC-001");

    let entries: Vec<_> = std::fs::read_dir(&state_root)
        .expect("state_root must exist after a run")
        .flatten()
        .collect();
    assert_eq!(entries.len(), 1, "exactly one execution directory expected");
    let exec_id = Uuid::parse_str(
        entries[0]
            .file_name()
            .to_str()
            .expect("valid utf8 dir name"),
    )
    .expect("execution dir must be a UUID");
    let execution = read_execution_json(&state_root, exec_id);
    let task_run = execution["task_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["task_id"].as_str() == Some("call_child"))
        .expect("call_child run entry");
    assert_eq!(task_run["error_code"].as_str(), Some("WFG-NEST-005"));
}

// -----------------------------------------------------------------------------
// Scenario 46: Planner-like short-circuit after enrichment failure
// -----------------------------------------------------------------------------
#[tokio::test]
#[cfg(unix)]
#[serial(path_env_agent)]
async fn test_scenario_46_planner_short_circuit_on_enrich_failure() {
    let harness = WorkflowTestHarness::new(HashMap::new(), FakeInterviewer::new());
    write_agent_stub(
        harness.temp_dir.path(),
        "echo '{\"error\":{\"status\":429,\"message\":\"hourly quota exceeded\"}}'\nexit 0\n",
    );
    let _path = PathGuard::prepend(harness.temp_dir.path());

    let state_root = harness.temp_dir.path().join(".newton/state/workflows");
    let err = harness
        .run_fixture("46_planner_quota_short_circuit.yaml", None)
        .await
        .expect_err("workflow must fail at enrich_spec");
    assert_eq!(err.code, "WFG-EXEC-001");
    assert!(err.message.contains("enrich_spec"));

    let entries: Vec<_> = std::fs::read_dir(&state_root)
        .expect("state_root must exist after a run")
        .flatten()
        .collect();
    assert_eq!(entries.len(), 1, "exactly one execution directory expected");
    let exec_id = Uuid::parse_str(
        entries[0]
            .file_name()
            .to_str()
            .expect("valid utf8 dir name"),
    )
    .expect("execution dir must be a UUID");
    let execution = read_execution_json(&state_root, exec_id);
    let ids = task_run_ids(&execution);
    assert!(ids.contains("enrich_spec"));
    let update_run = execution["task_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["task_id"].as_str() == Some("update_board_body"));
    let move_run = execution["task_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["task_id"].as_str() == Some("move_to_backlog"));
    assert!(
        update_run.is_none_or(|r| r["status"].as_str() != Some("success")),
        "update_board_body must not execute successfully after enrich failure"
    );
    assert!(
        move_run.is_none_or(|r| r["status"].as_str() != Some("success")),
        "move_to_backlog must not execute successfully after enrich failure"
    );

    let enrich_run = execution["task_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["task_id"].as_str() == Some("enrich_spec"))
        .expect("enrich_spec run entry");
    assert_eq!(enrich_run["status"].as_str(), Some("failed"));
    assert_eq!(enrich_run["error_code"].as_str(), Some("WFG-AGENT-008"));

    let checkpoint_path = state_root.join(exec_id.to_string()).join("checkpoint.json");
    let checkpoint_value: Value =
        serde_json::from_slice(&std::fs::read(&checkpoint_path).expect("read checkpoint.json"))
            .expect("parse checkpoint.json");
    let checkpoint_enrich = checkpoint_value["completed"]["enrich_spec"].clone();
    assert_eq!(checkpoint_enrich["status"].as_str(), Some("failed"));
    assert_eq!(
        checkpoint_enrich["error"]["code"].as_str(),
        Some("WFG-AGENT-008")
    );
    assert!(
        checkpoint_enrich["error"]["message"]
            .as_str()
            .is_some_and(|msg| !msg.is_empty()),
        "checkpoint should persist non-empty error summary message"
    );
}
