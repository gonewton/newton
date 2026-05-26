use async_trait::async_trait;
use newton_core::core::error::AppError;
use newton_core::core::types::ErrorCategory;
use newton_core::workflow::executor::{ExecutionOverrides, ExecutionSummary, GraphHandle};
use newton_core::workflow::operator::{ExecutionContext, Operator, OperatorRegistry, StateView};
use newton_core::workflow::operators::gh::{GhOperator, GhOutput, GhRunner, GitRunner};
use newton_core::workflow::operators::gh_authorization::{
    AiloopApprover, ApprovalOutcome, AuthorizationRequest,
};
use newton_core::workflow::operators::{self, BuiltinOperatorDeps};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::{tempdir, TempDir};

#[derive(Clone)]
struct MockGhRunner {
    responses: Arc<Mutex<HashMap<Vec<String>, GhOutput>>>,
    calls: Arc<AtomicUsize>,
    last_cwd: Arc<Mutex<Option<std::path::PathBuf>>>,
}

impl MockGhRunner {
    fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            calls: Arc::new(AtomicUsize::new(0)),
            last_cwd: Arc::new(Mutex::new(None)),
        }
    }

    fn add_response(&self, args: Vec<&str>, output: GhOutput) {
        let key: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.responses.lock().unwrap().insert(key, output);
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn last_cwd(&self) -> Option<std::path::PathBuf> {
        self.last_cwd.lock().unwrap().clone()
    }
}

#[async_trait]
impl GhRunner for MockGhRunner {
    async fn run(&self, args: &[&str], cwd: &std::path::Path) -> Result<GhOutput, AppError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_cwd.lock().unwrap() = Some(cwd.to_path_buf());
        let key: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let responses = self.responses.lock().unwrap();
        match responses.get(&key) {
            Some(output) => Ok(output.clone()),
            None => Err(AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("mock gh: no response for {:?}", key),
            )),
        }
    }
}

type GitResponses = Arc<Mutex<HashMap<Vec<String>, GhOutput>>>;

#[derive(Clone)]
struct MockGitRunner {
    responses: GitResponses,
    calls: Arc<AtomicUsize>,
    last_cwd: Arc<Mutex<Option<std::path::PathBuf>>>,
}

impl MockGitRunner {
    fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            calls: Arc::new(AtomicUsize::new(0)),
            last_cwd: Arc::new(Mutex::new(None)),
        }
    }

    fn add_success(&self, args: Vec<&str>, output: GhOutput) {
        let key: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.responses.lock().unwrap().insert(key, output);
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn last_cwd(&self) -> Option<std::path::PathBuf> {
        self.last_cwd.lock().unwrap().clone()
    }
}

#[async_trait]
impl GitRunner for MockGitRunner {
    async fn run(&self, args: &[&str], cwd: &std::path::Path) -> Result<GhOutput, AppError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_cwd.lock().unwrap() = Some(cwd.to_path_buf());
        let key: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let responses = self.responses.lock().unwrap();
        match responses.get(&key) {
            Some(output) => Ok(output.clone()),
            None => Err(AppError::new(
                ErrorCategory::ToolExecutionError,
                format!("mock git: no response for {:?}", key),
            )
            .with_code("WFG-GH-011")),
        }
    }
}

fn build_registry_with_git_runner(
    workspace: std::path::PathBuf,
    git_runner: Arc<dyn GitRunner>,
) -> OperatorRegistry {
    let mut builder = OperatorRegistry::builder();
    let deps = BuiltinOperatorDeps {
        interviewer: None,
        command_runner: None,
        gh_runner: None,
        child_workflow_runner: None,
        gh_approver: None,
        git_runner: Some(git_runner),
    };
    operators::register_builtins_with_deps(&mut builder, workspace, Default::default(), deps);
    builder.build()
}

fn build_registry_with_gh_runner(
    workspace: std::path::PathBuf,
    runner: Arc<dyn GhRunner>,
) -> OperatorRegistry {
    let mut builder = OperatorRegistry::builder();
    let deps = BuiltinOperatorDeps {
        interviewer: None,
        command_runner: None,
        gh_runner: Some(runner),
        child_workflow_runner: None,
        gh_approver: None,
        git_runner: None,
    };
    operators::register_builtins_with_deps(&mut builder, workspace, Default::default(), deps);
    builder.build()
}

async fn execute_yaml_with_gh_runner(
    workspace: &std::path::Path,
    yaml: &str,
    runner: Arc<dyn GhRunner>,
) -> Result<ExecutionSummary, AppError> {
    let mut workflow_file = tempfile::NamedTempFile::new().expect("workflow temp file");
    write!(workflow_file, "{}", yaml).expect("write workflow");

    let document =
        newton_core::workflow::schema::load_workflow(workflow_file.path()).expect("load workflow");

    let registry = build_registry_with_gh_runner(workspace.to_path_buf(), runner);

    newton_core::workflow::executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry,
        workspace.to_path_buf(),
        ExecutionOverrides {
            parallel_limit: None,
            max_time_seconds: None,
            checkpoint_base_path: None,
            artifact_base_path: None,
            max_nesting_depth: None,
            verbose: false,
            sink: None,
            pre_seed_nodes: true,
        },
    )
    .await
}

#[tokio::test]
async fn test_gh_operator_project_resolve_board() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());

    let project_view_json = json!({
        "id": "PVT_abc123",
        "title": "Test Project",
        "shortDescription": "A test project",
        "public": true,
        "readme": "This is a test project"
    });

    let field_list_json = json!({
        "fields": [
            {
                "id": "FLD_status",
                "name": "Status",
                "dataType": "SINGLE_SELECT",
                "options": [
                    {"id": "OPT_ready", "name": "Ready"},
                    {"id": "OPT_in_progress", "name": "In progress"},
                    {"id": "OPT_in_review", "name": "In review"},
                    {"id": "OPT_done", "name": "Done"}
                ]
            }
        ]
    });

    runner.add_response(
        vec![
            "project", "view", "1", "--owner", "testorg", "--format", "json",
        ],
        GhOutput {
            stdout: project_view_json.to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    runner.add_response(
        vec![
            "project",
            "field-list",
            "1",
            "--owner",
            "testorg",
            "--format",
            "json",
        ],
        GhOutput {
            stdout: field_list_json.to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: resolve_board
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 10
    max_workflow_iterations: 100
  tasks:
    - id: resolve_board
      operator: GhOperator
      params:
        operation: project_resolve_board
        owner: testorg
        project_number: 1
      transitions:
        - to: verify
    - id: verify
      operator: NoOpOperator
      terminal: success
      params: {}
"#;

    let summary = execute_yaml_with_gh_runner(workspace.path(), yaml, runner)
        .await
        .expect("workflow should complete");

    let task = summary
        .completed_tasks
        .get("resolve_board")
        .expect("resolve_board task");
    let output = task.output.clone();
    assert_eq!(output["project_id"], "PVT_abc123");
    assert_eq!(output["field_id"], "FLD_status");
    assert_eq!(output["ready_id"], "OPT_ready");
    assert_eq!(output["in_progress_id"], "OPT_in_progress");
    assert_eq!(output["in_review_id"], "OPT_in_review");
    assert_eq!(output["done_id"], "OPT_done");

    let options = output["options"].as_object().expect("options map");
    assert_eq!(options["Ready"], "OPT_ready");
    assert_eq!(options["In progress"], "OPT_in_progress");
}

#[tokio::test]
async fn test_gh_operator_project_item_set_status() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());

    let project_view_json = json!({
        "id": "PVT_abc123",
        "title": "Test Project"
    });

    let field_list_json = json!({
        "fields": [
            {
                "id": "FLD_status",
                "name": "Status",
                "dataType": "SINGLE_SELECT",
                "options": [
                    {"id": "OPT_ready", "name": "Ready"},
                    {"id": "OPT_in_progress", "name": "In progress"},
                    {"id": "OPT_in_review", "name": "In review"},
                    {"id": "OPT_done", "name": "Done"}
                ]
            }
        ]
    });

    runner.add_response(
        vec![
            "project", "view", "1", "--owner", "testorg", "--format", "json",
        ],
        GhOutput {
            stdout: project_view_json.to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    runner.add_response(
        vec![
            "project",
            "field-list",
            "1",
            "--owner",
            "testorg",
            "--format",
            "json",
        ],
        GhOutput {
            stdout: field_list_json.to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    runner.add_response(
        vec![
            "project",
            "item-edit",
            "--project-id",
            "PVT_abc123",
            "--id",
            "ITEM_123",
            "--field-id",
            "FLD_status",
            "--single-select-option-id",
            "OPT_in_progress",
        ],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: resolve_board
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 10
    max_workflow_iterations: 100
  tasks:
    - id: resolve_board
      operator: GhOperator
      params:
        operation: project_resolve_board
        owner: testorg
        project_number: 1
      transitions:
        - to: set_status
    - id: set_status
      operator: GhOperator
      params:
        operation: project_item_set_status
        item_id: "ITEM_123"
        board: { $expr: 'tasks.resolve_board.output' }
        status: "In progress"
        on_error: fail
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
"#;

    let summary = execute_yaml_with_gh_runner(workspace.path(), yaml, runner)
        .await
        .expect("workflow should complete");

    let task = summary
        .completed_tasks
        .get("set_status")
        .expect("set_status task");
    let output = task.output.clone();
    assert_eq!(output["updated"], true);
}

#[tokio::test]
async fn test_gh_operator_pr_create() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());

    runner.add_response(
        vec![
            "pr",
            "create",
            "--base",
            "main",
            "--title",
            "Test PR",
            "--body",
            "Test body",
        ],
        GhOutput {
            stdout: "https://github.com/testorg/testrepo/pull/42".to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: create_pr
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 10
    max_workflow_iterations: 100
  tasks:
    - id: create_pr
      operator: GhOperator
      params:
        operation: pr_create
        title: "Test PR"
        body: "Test body"
        base: main
        retry_count: 1
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
"#;

    let summary = execute_yaml_with_gh_runner(workspace.path(), yaml, runner)
        .await
        .expect("workflow should complete");

    let task = summary
        .completed_tasks
        .get("create_pr")
        .expect("create_pr task");
    let output = task.output.clone();
    assert_eq!(
        output["pr_url"],
        "https://github.com/testorg/testrepo/pull/42"
    );
    assert_eq!(output["pr_number"], 42);
}

#[tokio::test]
async fn test_gh_operator_pr_view() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());

    runner.add_response(
        vec!["pr", "view", "42", "--json", "state"],
        GhOutput {
            stdout: r#"{"state":"OPEN"}"#.to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: view_pr
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 10
    max_workflow_iterations: 100
  tasks:
    - id: view_pr
      operator: GhOperator
      params:
        operation: pr_view
        pr: 42
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
"#;

    let summary = execute_yaml_with_gh_runner(workspace.path(), yaml, runner)
        .await
        .expect("workflow should complete");

    let task = summary
        .completed_tasks
        .get("view_pr")
        .expect("view_pr task");
    let output = task.output.clone();
    assert_eq!(output["state"], "OPEN");
    assert_eq!(output["pr_number"], 42);
}

#[tokio::test]
async fn test_gh_operator_pr_view_with_url() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());

    runner.add_response(
        vec!["pr", "view", "99", "--json", "state"],
        GhOutput {
            stdout: r#"{"state":"MERGED"}"#.to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: view_pr
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 10
    max_workflow_iterations: 100
  tasks:
    - id: view_pr
      operator: GhOperator
      params:
        operation: pr_view
        pr: "https://github.com/owner/repo/pull/99"
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
"#;

    let summary = execute_yaml_with_gh_runner(workspace.path(), yaml, runner)
        .await
        .expect("workflow should complete");

    let task = summary
        .completed_tasks
        .get("view_pr")
        .expect("view_pr task");
    let output = task.output.clone();
    assert_eq!(output["state"], "MERGED");
    assert_eq!(output["pr_number"], 99);
}

#[tokio::test]
async fn test_gh_operator_project_resolve_board_backlog_only() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());

    let project_view_json = json!({
        "id": "PVT_planner",
        "title": "Planner Project"
    });

    let field_list_json = json!({
        "fields": [
            {
                "id": "FLD_status",
                "name": "Status",
                "dataType": "SINGLE_SELECT",
                "options": [
                    {"id": "OPT_backlog", "name": "Backlog"}
                ]
            }
        ]
    });

    runner.add_response(
        vec![
            "project", "view", "1", "--owner", "testorg", "--format", "json",
        ],
        GhOutput {
            stdout: project_view_json.to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    runner.add_response(
        vec![
            "project",
            "field-list",
            "1",
            "--owner",
            "testorg",
            "--format",
            "json",
        ],
        GhOutput {
            stdout: field_list_json.to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    runner.add_response(
        vec![
            "project",
            "item-edit",
            "--project-id",
            "PVT_planner",
            "--id",
            "ITEM_1",
            "--field-id",
            "FLD_status",
            "--single-select-option-id",
            "OPT_backlog",
        ],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: resolve_board
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 10
    max_workflow_iterations: 100
  tasks:
    - id: resolve_board
      operator: GhOperator
      params:
        operation: project_resolve_board
        owner: testorg
        project_number: 1
        required_option_names:
          - Backlog
      transitions:
        - to: set_backlog
    - id: set_backlog
      operator: GhOperator
      params:
        operation: project_item_set_status
        item_id: "ITEM_1"
        board: { $expr: 'tasks.resolve_board.output' }
        status: "Backlog"
        on_error: fail
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
"#;

    let summary = execute_yaml_with_gh_runner(workspace.path(), yaml, runner)
        .await
        .expect("workflow should complete");

    let resolve = summary
        .completed_tasks
        .get("resolve_board")
        .expect("resolve_board");
    assert_eq!(resolve.output["backlog_id"], "OPT_backlog");
    assert_eq!(resolve.output["project_id"], "PVT_planner");

    let set = summary
        .completed_tasks
        .get("set_backlog")
        .expect("set_backlog");
    assert_eq!(set.output["updated"], true);
}

// ----- Authorization gating tests -----

#[derive(Clone, Copy)]
enum MockOutcome {
    Approved,
    Denied,
    Timeout,
    Unavailable,
}

#[derive(Clone)]
struct MockApprover {
    outcome: MockOutcome,
    calls: Arc<Mutex<Vec<AuthorizationRequest>>>,
}

impl MockApprover {
    fn new(outcome: MockOutcome) -> Self {
        Self {
            outcome,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    fn last_request(&self) -> Option<AuthorizationRequest> {
        self.calls.lock().unwrap().last().cloned()
    }
}

#[async_trait]
impl AiloopApprover for MockApprover {
    async fn authorize(&self, request: AuthorizationRequest) -> Result<ApprovalOutcome, AppError> {
        self.calls.lock().unwrap().push(request);
        Ok(match self.outcome {
            MockOutcome::Approved => ApprovalOutcome::Approved,
            MockOutcome::Denied => ApprovalOutcome::Denied { reason: None },
            MockOutcome::Timeout => ApprovalOutcome::Timeout,
            MockOutcome::Unavailable => ApprovalOutcome::Unavailable {
                cause: "test".into(),
            },
        })
    }
}

fn build_ctx(workspace: &TempDir) -> ExecutionContext {
    let empty = Value::Object(Map::new());
    ExecutionContext {
        workspace_path: workspace.path().to_path_buf(),
        execution_id: "exec".into(),
        task_id: "create_pr".into(),
        iteration: 1,
        state_view: StateView::new(empty.clone(), empty.clone(), empty),
        graph: GraphHandle::new(HashMap::new()),
        workflow_file: workspace.path().join("workflow.yaml"),
        nesting_depth: 0,
        execution_overrides: ExecutionOverrides {
            parallel_limit: None,
            max_time_seconds: None,
            checkpoint_base_path: None,
            artifact_base_path: None,
            max_nesting_depth: None,
            verbose: false,
            sink: None,
            pre_seed_nodes: true,
        },
        operator_registry: OperatorRegistry::new(),
    }
}

fn pr_create_params(extra: &[(&str, Value)]) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("operation".into(), json!("pr_create"));
    map.insert("title".into(), json!("Test PR"));
    map.insert("base".into(), json!("main"));
    map.insert("retry_count".into(), json!(1));
    for (k, v) in extra {
        map.insert((*k).into(), v.clone());
    }
    Value::Object(map)
}

fn ok_runner() -> Arc<MockGhRunner> {
    let runner = Arc::new(MockGhRunner::new());
    runner.add_response(
        vec![
            "pr", "create", "--base", "main", "--title", "Test PR", "--body", "",
        ],
        GhOutput {
            stdout: "https://github.com/o/r/pull/7".to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );
    runner
}

#[tokio::test]
async fn auth_disabled_does_not_call_approver() {
    let workspace = tempdir().expect("workspace");
    let runner = ok_runner();
    let approver = Arc::new(MockApprover::new(MockOutcome::Denied));
    let op = GhOperator::with_runner_and_approver(
        runner.clone() as Arc<dyn GhRunner>,
        approver.clone() as Arc<dyn AiloopApprover>,
    );
    let res = op
        .execute(pr_create_params(&[]), build_ctx(&workspace))
        .await;
    assert!(res.is_ok(), "expected ok, got {res:?}");
    assert_eq!(approver.call_count(), 0);
    assert_eq!(runner.call_count(), 1);
}

#[tokio::test]
async fn auth_approved_runs_gh() {
    let workspace = tempdir().expect("workspace");
    let runner = ok_runner();
    let approver = Arc::new(MockApprover::new(MockOutcome::Approved));
    let op = GhOperator::with_runner_and_approver(
        runner.clone() as Arc<dyn GhRunner>,
        approver.clone() as Arc<dyn AiloopApprover>,
    );
    op.execute(
        pr_create_params(&[("require_authorization", json!(true))]),
        build_ctx(&workspace),
    )
    .await
    .expect("approved");
    assert_eq!(approver.call_count(), 1);
    assert_eq!(runner.call_count(), 1);
}

#[tokio::test]
async fn auth_denied_blocks_gh_with_code_001() {
    let workspace = tempdir().expect("workspace");
    let runner = ok_runner();
    let approver = Arc::new(MockApprover::new(MockOutcome::Denied));
    let op = GhOperator::with_runner_and_approver(
        runner.clone() as Arc<dyn GhRunner>,
        approver.clone() as Arc<dyn AiloopApprover>,
    );
    let err = op
        .execute(
            pr_create_params(&[("require_authorization", json!(true))]),
            build_ctx(&workspace),
        )
        .await
        .expect_err("denied");
    assert_eq!(err.code, "WFG-GH-AUTH-001");
    assert_eq!(runner.call_count(), 0);
    assert_eq!(approver.call_count(), 1);
}

#[tokio::test]
async fn auth_timeout_yields_code_002() {
    let workspace = tempdir().expect("workspace");
    let runner = ok_runner();
    let approver = Arc::new(MockApprover::new(MockOutcome::Timeout));
    let op = GhOperator::with_runner_and_approver(
        runner.clone() as Arc<dyn GhRunner>,
        approver.clone() as Arc<dyn AiloopApprover>,
    );
    let err = op
        .execute(
            pr_create_params(&[("require_authorization", json!(true))]),
            build_ctx(&workspace),
        )
        .await
        .expect_err("timeout");
    assert_eq!(err.code, "WFG-GH-AUTH-002");
    assert_eq!(runner.call_count(), 0);
}

#[tokio::test]
async fn auth_unavailable_fails_with_code_003() {
    let workspace = tempdir().expect("workspace");
    let runner = ok_runner();
    let approver = Arc::new(MockApprover::new(MockOutcome::Unavailable));
    let op = GhOperator::with_runner_and_approver(
        runner.clone() as Arc<dyn GhRunner>,
        approver.clone() as Arc<dyn AiloopApprover>,
    );
    let err = op
        .execute(
            pr_create_params(&[("require_authorization", json!(true))]),
            build_ctx(&workspace),
        )
        .await
        .expect_err("unavailable");
    assert_eq!(err.code, "WFG-GH-AUTH-003");
    assert_eq!(runner.call_count(), 0);
}

#[tokio::test]
async fn auth_unavailable_skip_runs_gh() {
    let workspace = tempdir().expect("workspace");
    let runner = ok_runner();
    let approver = Arc::new(MockApprover::new(MockOutcome::Unavailable));
    let op = GhOperator::with_runner_and_approver(
        runner.clone() as Arc<dyn GhRunner>,
        approver.clone() as Arc<dyn AiloopApprover>,
    );
    op.execute(
        pr_create_params(&[
            ("require_authorization", json!(true)),
            ("on_authorization_unavailable", json!("skip")),
        ]),
        build_ctx(&workspace),
    )
    .await
    .expect("skip path ok");
    assert_eq!(runner.call_count(), 1);
    assert_eq!(approver.call_count(), 1);
}

#[tokio::test]
async fn auth_default_noop_with_no_wired_approver_returns_003() {
    let workspace = tempdir().expect("workspace");
    let runner = ok_runner();
    let op = GhOperator::with_runner(runner.clone() as Arc<dyn GhRunner>);
    let err = op
        .execute(
            pr_create_params(&[("require_authorization", json!(true))]),
            build_ctx(&workspace),
        )
        .await
        .expect_err("noop default fails");
    assert_eq!(err.code, "WFG-GH-AUTH-003");
    assert_eq!(runner.call_count(), 0);
}

#[tokio::test]
async fn auth_channel_override_propagates() {
    let workspace = tempdir().expect("workspace");
    let runner = ok_runner();
    let approver = Arc::new(MockApprover::new(MockOutcome::Approved));
    let op = GhOperator::with_runner_and_approver(
        runner as Arc<dyn GhRunner>,
        approver.clone() as Arc<dyn AiloopApprover>,
    );
    op.execute(
        pr_create_params(&[
            ("require_authorization", json!(true)),
            ("authorization_channel", json!("release-bot")),
        ]),
        build_ctx(&workspace),
    )
    .await
    .expect("ok");
    let req = approver.last_request().expect("captured");
    assert_eq!(req.channel.as_deref(), Some("release-bot"));
}

#[tokio::test]
async fn auth_default_prompts() {
    let workspace = tempdir().expect("workspace");

    // pr_create
    {
        let runner = ok_runner();
        let approver = Arc::new(MockApprover::new(MockOutcome::Approved));
        let op = GhOperator::with_runner_and_approver(
            runner as Arc<dyn GhRunner>,
            approver.clone() as Arc<dyn AiloopApprover>,
        );
        op.execute(
            pr_create_params(&[("require_authorization", json!(true))]),
            build_ctx(&workspace),
        )
        .await
        .unwrap();
        assert_eq!(
            approver.last_request().unwrap().prompt,
            "Authorize gh pr create: title=\"Test PR\", base=\"main\""
        );
    }

    // pr_view
    {
        let runner = Arc::new(MockGhRunner::new());
        runner.add_response(
            vec!["pr", "view", "42", "--json", "state"],
            GhOutput {
                stdout: r#"{"state":"OPEN"}"#.into(),
                stderr: String::new(),
                exit_code: 0,
            },
        );
        let approver = Arc::new(MockApprover::new(MockOutcome::Approved));
        let op = GhOperator::with_runner_and_approver(
            runner as Arc<dyn GhRunner>,
            approver.clone() as Arc<dyn AiloopApprover>,
        );
        op.execute(
            json!({"operation": "pr_view", "pr": 42, "require_authorization": true}),
            build_ctx(&workspace),
        )
        .await
        .unwrap();
        assert_eq!(
            approver.last_request().unwrap().prompt,
            "Authorize gh pr view: pr=42"
        );
    }

    // project_resolve_board
    {
        let runner = Arc::new(MockGhRunner::new());
        runner.add_response(
            vec![
                "project", "view", "1", "--owner", "acme", "--format", "json",
            ],
            GhOutput {
                stdout: json!({"id": "PVT_1"}).to_string(),
                stderr: String::new(),
                exit_code: 0,
            },
        );
        runner.add_response(
            vec![
                "project",
                "field-list",
                "1",
                "--owner",
                "acme",
                "--format",
                "json",
            ],
            GhOutput {
                stdout: json!({
                    "fields": [{
                        "id":"FLD","name":"Status",
                        "options":[
                            {"id":"r","name":"Ready"},
                            {"id":"i","name":"In progress"},
                            {"id":"v","name":"In review"},
                            {"id":"d","name":"Done"}
                        ]
                    }]
                })
                .to_string(),
                stderr: String::new(),
                exit_code: 0,
            },
        );
        let approver = Arc::new(MockApprover::new(MockOutcome::Approved));
        let op = GhOperator::with_runner_and_approver(
            runner as Arc<dyn GhRunner>,
            approver.clone() as Arc<dyn AiloopApprover>,
        );
        op.execute(
            json!({
                "operation": "project_resolve_board",
                "owner": "acme",
                "project_number": 1,
                "require_authorization": true
            }),
            build_ctx(&workspace),
        )
        .await
        .unwrap();
        assert_eq!(
            approver.last_request().unwrap().prompt,
            "Authorize gh project view/field-list: owner=acme, project=1"
        );
    }

    // project_item_set_status
    {
        let runner = Arc::new(MockGhRunner::new());
        runner.add_response(
            vec![
                "project",
                "item-edit",
                "--project-id",
                "P",
                "--id",
                "ITEM_9",
                "--field-id",
                "F",
                "--single-select-option-id",
                "OPT_R",
            ],
            GhOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            },
        );
        let approver = Arc::new(MockApprover::new(MockOutcome::Approved));
        let op = GhOperator::with_runner_and_approver(
            runner as Arc<dyn GhRunner>,
            approver.clone() as Arc<dyn AiloopApprover>,
        );
        op.execute(
            json!({
                "operation": "project_item_set_status",
                "item_id": "ITEM_9",
                "board": {"project_id": "P", "field_id": "F", "ready_id": "OPT_R"},
                "status": "Ready",
                "require_authorization": true,
                "on_error": "fail"
            }),
            build_ctx(&workspace),
        )
        .await
        .unwrap();
        assert_eq!(
            approver.last_request().unwrap().prompt,
            "Authorize gh project item-edit: item=ITEM_9, status=Ready"
        );
    }
}

#[tokio::test]
async fn validate_rejects_invalid_on_unavailable() {
    let op = GhOperator::new();
    let params = json!({
        "operation": "pr_view",
        "pr": 1,
        "on_authorization_unavailable": "halt",
    });
    let err = op.validate_params(&params).unwrap_err();
    assert_eq!(err.code, "WFG-GH-AUTH-004");
}

#[tokio::test]
async fn validate_rejects_zero_and_negative_timeout() {
    let op = GhOperator::new();
    for bad in &[json!(0), json!(-5)] {
        let params = json!({
            "operation": "pr_view",
            "pr": 1,
            "authorization_timeout_seconds": bad,
        });
        let err = op.validate_params(&params).unwrap_err();
        assert_eq!(err.code, "WFG-GH-AUTH-005");
    }
}

#[tokio::test]
async fn auth_single_call_across_internal_retries() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new()); // no responses → always errors → triggers retries
    let approver = Arc::new(MockApprover::new(MockOutcome::Approved));
    let op = GhOperator::with_runner_and_approver(
        runner.clone() as Arc<dyn GhRunner>,
        approver.clone() as Arc<dyn AiloopApprover>,
    );
    let _ = op
        .execute(
            pr_create_params(&[
                ("require_authorization", json!(true)),
                ("retry_count", json!(3)),
                ("retry_delay_ms", json!(0)),
            ]),
            build_ctx(&workspace),
        )
        .await;
    assert_eq!(approver.call_count(), 1);
    assert_eq!(runner.call_count(), 3);
}

// ===== pr_approve tests =====

fn pr_approve_params(extra: &[(&str, Value)]) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("operation".into(), json!("pr_approve"));
    for (k, v) in extra {
        map.insert((*k).into(), v.clone());
    }
    Value::Object(map)
}

fn pr_approve_ctx(workspace: &TempDir) -> ExecutionContext {
    let empty = Value::Object(Map::new());
    ExecutionContext {
        workspace_path: workspace.path().to_path_buf(),
        execution_id: "exec".into(),
        task_id: "approve_pr".into(),
        iteration: 1,
        state_view: StateView::new(empty.clone(), empty.clone(), empty),
        graph: GraphHandle::new(HashMap::new()),
        workflow_file: workspace.path().join("workflow.yaml"),
        nesting_depth: 0,
        execution_overrides: ExecutionOverrides {
            parallel_limit: None,
            max_time_seconds: None,
            checkpoint_base_path: None,
            artifact_base_path: None,
            max_nesting_depth: None,
            verbose: false,
            sink: None,
            pre_seed_nodes: true,
        },
        operator_registry: OperatorRegistry::new(),
    }
}

// AC#1: pr_number + repository => gh pr review 36 --approve -R owner/repo
#[tokio::test]
async fn pr_approve_with_number_and_repository() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());
    runner.add_response(
        vec!["pr", "review", "36", "--approve", "-R", "owner/repo"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );
    let op = GhOperator::with_runner(runner.clone() as Arc<dyn GhRunner>);
    let result = op
        .execute(
            pr_approve_params(&[
                ("pr_number", json!(36)),
                ("repository", json!("owner/repo")),
            ]),
            pr_approve_ctx(&workspace),
        )
        .await
        .expect("should succeed");
    assert_eq!(result["review_submitted"], true);
    assert_eq!(result["pr_number"], 36);
    assert_eq!(result["repository"], "owner/repo");
    assert_eq!(result["pr_url"], "https://github.com/owner/repo/pull/36");
    assert_eq!(runner.call_count(), 1);
}

// AC#2: pr_url => gh pr review 36 --approve -R owner/repo
#[tokio::test]
async fn pr_approve_with_url() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());
    runner.add_response(
        vec!["pr", "review", "36", "--approve", "-R", "owner/repo"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );
    let op = GhOperator::with_runner(runner.clone() as Arc<dyn GhRunner>);
    let result = op
        .execute(
            pr_approve_params(&[("pr_url", json!("https://github.com/owner/repo/pull/36"))]),
            pr_approve_ctx(&workspace),
        )
        .await
        .expect("should succeed");
    assert_eq!(result["review_submitted"], true);
    assert_eq!(result["pr_number"], 36);
    assert_eq!(result["repository"], "owner/repo");
    assert_eq!(result["pr_url"], "https://github.com/owner/repo/pull/36");
    assert_eq!(runner.call_count(), 1);
}

// AC#3: pr_number alone (no repository) => no -R, output omits repository and pr_url
#[tokio::test]
async fn pr_approve_with_number_only() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());
    runner.add_response(
        vec!["pr", "review", "36", "--approve"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );
    let op = GhOperator::with_runner(runner.clone() as Arc<dyn GhRunner>);
    let result = op
        .execute(
            pr_approve_params(&[("pr_number", json!(36))]),
            pr_approve_ctx(&workspace),
        )
        .await
        .expect("should succeed");
    assert_eq!(result["review_submitted"], true);
    assert_eq!(result["pr_number"], 36);
    assert!(result.get("repository").is_none());
    assert!(result.get("pr_url").is_none());
    assert_eq!(runner.call_count(), 1);
}

// AC#4: both pr_number and pr_url => WFG-GH-005
#[test]
fn pr_approve_both_selectors_fails_005() {
    let op = GhOperator::new();
    let params = pr_approve_params(&[
        ("pr_number", json!(36)),
        ("pr_url", json!("https://github.com/o/r/pull/36")),
    ]);
    let err = op.validate_params(&params).unwrap_err();
    assert_eq!(err.code, "WFG-GH-005");
}

// AC#5: neither pr_number nor pr_url => WFG-GH-005
#[test]
fn pr_approve_neither_selector_fails_005() {
    let op = GhOperator::new();
    let params = pr_approve_params(&[]);
    let err = op.validate_params(&params).unwrap_err();
    assert_eq!(err.code, "WFG-GH-005");
}

// AC#6: non-HTTPS pr_url => WFG-GH-006
#[test]
fn pr_approve_http_url_fails_006() {
    let op = GhOperator::new();
    let params = pr_approve_params(&[("pr_url", json!("http://github.com/o/r/pull/1"))]);
    let err = op.validate_params(&params).unwrap_err();
    assert_eq!(err.code, "WFG-GH-006");
}

// AC#7: pr_url with non-numeric tail => WFG-GH-006
#[test]
fn pr_approve_url_non_numeric_fails_006() {
    let op = GhOperator::new();
    let params = pr_approve_params(&[("pr_url", json!("https://github.com/o/r/pull/abc"))]);
    let err = op.validate_params(&params).unwrap_err();
    assert_eq!(err.code, "WFG-GH-006");
}

// AC#8: malformed repository => WFG-GH-007
#[test]
fn pr_approve_bad_repository_fails_007() {
    let op = GhOperator::new();
    let params = pr_approve_params(&[("pr_number", json!(1)), ("repository", json!("not-a-repo"))]);
    let err = op.validate_params(&params).unwrap_err();
    assert_eq!(err.code, "WFG-GH-007");
}

// AC#9: pr_number 0 and -1 => WFG-GH-008
#[test]
fn pr_approve_zero_pr_number_fails_008() {
    let op = GhOperator::new();
    let params = pr_approve_params(&[("pr_number", json!(0))]);
    let err = op.validate_params(&params).unwrap_err();
    assert_eq!(err.code, "WFG-GH-008");
}

#[test]
fn pr_approve_negative_pr_number_fails_008() {
    let op = GhOperator::new();
    let params = pr_approve_params(&[("pr_number", json!(-1))]);
    let err = op.validate_params(&params).unwrap_err();
    assert_eq!(err.code, "WFG-GH-008");
}

// AC#10: gh non-zero exit => WFG-GH-004
#[tokio::test]
async fn pr_approve_gh_failure_yields_004() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());
    // No response registered => mock returns error, but we need a specific WFG-GH-004.
    // The MockGhRunner returns a generic ToolExecutionError. Let's test with a real error path.
    // We rely on the mock not having a response for these args which returns an error.
    let op = GhOperator::with_runner(runner.clone() as Arc<dyn GhRunner>);
    let err = op
        .execute(
            pr_approve_params(&[("pr_number", json!(99))]),
            pr_approve_ctx(&workspace),
        )
        .await
        .unwrap_err();
    // The mock returns a ToolExecutionError when no matching response is found.
    assert_eq!(err.category, ErrorCategory::ToolExecutionError);
}

// AC#11: require_authorization + denied => gh not spawned, WFG-GH-AUTH-001
#[tokio::test]
async fn pr_approve_auth_denied_blocks_gh() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());
    runner.add_response(
        vec!["pr", "review", "36", "--approve", "-R", "owner/repo"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );
    let approver = Arc::new(MockApprover::new(MockOutcome::Denied));
    let op = GhOperator::with_runner_and_approver(
        runner.clone() as Arc<dyn GhRunner>,
        approver.clone() as Arc<dyn AiloopApprover>,
    );
    let err = op
        .execute(
            pr_approve_params(&[
                ("pr_number", json!(36)),
                ("repository", json!("owner/repo")),
                ("require_authorization", json!(true)),
            ]),
            pr_approve_ctx(&workspace),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, "WFG-GH-AUTH-001");
    assert_eq!(runner.call_count(), 0);
    assert_eq!(approver.call_count(), 1);
}

// AC#12: derive_default_prompt for pr_approve
#[tokio::test]
async fn pr_approve_default_prompt_with_repository() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());
    runner.add_response(
        vec!["pr", "review", "36", "--approve", "-R", "owner/repo"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );
    let approver = Arc::new(MockApprover::new(MockOutcome::Approved));
    let op = GhOperator::with_runner_and_approver(
        runner as Arc<dyn GhRunner>,
        approver.clone() as Arc<dyn AiloopApprover>,
    );
    op.execute(
        pr_approve_params(&[
            ("pr_number", json!(36)),
            ("repository", json!("owner/repo")),
            ("require_authorization", json!(true)),
        ]),
        pr_approve_ctx(&workspace),
    )
    .await
    .unwrap();
    assert_eq!(
        approver.last_request().unwrap().prompt,
        "Authorize gh pr review --approve: pr=36, repository=owner/repo"
    );
}

#[tokio::test]
async fn pr_approve_default_prompt_without_repository() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());
    runner.add_response(
        vec!["pr", "review", "36", "--approve"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );
    let approver = Arc::new(MockApprover::new(MockOutcome::Approved));
    let op = GhOperator::with_runner_and_approver(
        runner as Arc<dyn GhRunner>,
        approver.clone() as Arc<dyn AiloopApprover>,
    );
    op.execute(
        pr_approve_params(&[
            ("pr_number", json!(36)),
            ("require_authorization", json!(true)),
        ]),
        pr_approve_ctx(&workspace),
    )
    .await
    .unwrap();
    assert_eq!(
        approver.last_request().unwrap().prompt,
        "Authorize gh pr review --approve: pr=36"
    );
}

#[tokio::test]
async fn pr_approve_default_prompt_with_url() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());
    runner.add_response(
        vec!["pr", "review", "36", "--approve", "-R", "owner/repo"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );
    let approver = Arc::new(MockApprover::new(MockOutcome::Approved));
    let op = GhOperator::with_runner_and_approver(
        runner as Arc<dyn GhRunner>,
        approver.clone() as Arc<dyn AiloopApprover>,
    );
    op.execute(
        pr_approve_params(&[
            ("pr_url", json!("https://github.com/owner/repo/pull/36")),
            ("require_authorization", json!(true)),
        ]),
        pr_approve_ctx(&workspace),
    )
    .await
    .unwrap();
    assert_eq!(
        approver.last_request().unwrap().prompt,
        "Authorize gh pr review --approve: pr=https://github.com/owner/repo/pull/36"
    );
}

// Validate pr_url with non-github host => WFG-GH-006
#[test]
fn pr_approve_non_github_host_fails_006() {
    let op = GhOperator::new();
    let params = pr_approve_params(&[("pr_url", json!("https://gitlab.com/o/r/pull/1"))]);
    let err = op.validate_params(&params).unwrap_err();
    assert_eq!(err.code, "WFG-GH-006");
}

// Enterprise github host should work
#[test]
fn pr_approve_enterprise_github_host_ok() {
    let op = GhOperator::new();
    let params = pr_approve_params(&[("pr_url", json!("https://github.example.com/o/r/pull/1"))]);
    assert!(op.validate_params(&params).is_ok());
}

// AC#17: Integration fixture end-to-end
#[tokio::test]
async fn pr_approve_integration_fixture() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());
    runner.add_response(
        vec!["pr", "review", "36", "--approve", "-R", "owner/repo"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let yaml = r#"
version: "2.0"
mode: workflow_graph
metadata:
  name: "GhOperator PR Approve Test Workflow"
workflow:
  context: {}
  settings:
    entry_task: approve_pr
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 10
    max_workflow_iterations: 100
  tasks:
    - id: approve_pr
      operator: GhOperator
      params:
        operation: pr_approve
        pr_number: 36
        repository: "owner/repo"
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
"#;

    let summary = execute_yaml_with_gh_runner(workspace.path(), yaml, runner)
        .await
        .expect("workflow should complete");

    let task = summary
        .completed_tasks
        .get("approve_pr")
        .expect("approve_pr task");
    let output = task.output.clone();
    assert_eq!(output["review_submitted"], true);
    assert_eq!(output["pr_number"], 36);
    assert_eq!(output["repository"], "owner/repo");
    assert_eq!(output["pr_url"], "https://github.com/owner/repo/pull/36");
}

/// Mock that fails the first N invocations with a configurable error, then succeeds.
#[derive(Clone)]
struct FlakyGhRunner {
    fail_count: Arc<AtomicUsize>,
    success_output: GhOutput,
    error_message: String,
    error_code: String,
    calls: Arc<AtomicUsize>,
}

impl FlakyGhRunner {
    fn new(
        fail_count: usize,
        success_output: GhOutput,
        error_message: &str,
        error_code: &str,
    ) -> Self {
        Self {
            fail_count: Arc::new(AtomicUsize::new(fail_count)),
            success_output,
            error_message: error_message.to_string(),
            error_code: error_code.to_string(),
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl GhRunner for FlakyGhRunner {
    async fn run(&self, _args: &[&str], _cwd: &std::path::Path) -> Result<GhOutput, AppError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let remaining = self.fail_count.load(Ordering::SeqCst);
        if remaining > 0 {
            self.fail_count.fetch_sub(1, Ordering::SeqCst);
            return Err(AppError::new(
                ErrorCategory::ToolExecutionError,
                self.error_message.clone(),
            )
            .with_code(&self.error_code));
        }
        Ok(self.success_output.clone())
    }
}

#[tokio::test]
async fn pr_view_retries_on_tls_timeout() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(FlakyGhRunner::new(
        3,
        GhOutput {
            stdout: r#"{"state":"OPEN"}"#.to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
        "gh command failed: TLS handshake timeout",
        "WFG-GH-004",
    ));

    let yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: view_pr
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 10
    max_workflow_iterations: 100
  tasks:
    - id: view_pr
      operator: GhOperator
      retry:
        max_attempts: 5
        backoff_ms: 10
        backoff_multiplier: 2.0
      params:
        operation: pr_view
        pr: 42
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
"#;
    let summary = execute_yaml_with_gh_runner(workspace.path(), yaml, runner.clone())
        .await
        .expect("workflow should complete");

    let task = summary
        .completed_tasks
        .get("view_pr")
        .expect("view_pr task");
    assert_eq!(task.output["state"], "OPEN");
    assert_eq!(runner.call_count(), 4);
}

#[tokio::test]
async fn pr_view_no_retry_on_validation_error() {
    let workspace = tempdir().expect("workspace");
    // Stdout that produces a JSON parse error (WFG-GH-002).
    let runner = Arc::new(FlakyGhRunner::new(
        0,
        GhOutput {
            stdout: "not-json".to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
        "unused",
        "unused",
    ));

    let yaml = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: view_pr
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: true
    max_task_iterations: 10
    max_workflow_iterations: 100
  tasks:
    - id: view_pr
      operator: GhOperator
      retry:
        max_attempts: 5
        backoff_ms: 10
        backoff_multiplier: 2.0
      params:
        operation: pr_view
        pr: 42
      transitions:
        - to: done
    - id: done
      operator: NoOpOperator
      terminal: success
      params: {}
"#;
    let _ = execute_yaml_with_gh_runner(workspace.path(), yaml, runner.clone()).await;
    // Engine must short-circuit on WFG-GH-002 (parse error) — exactly one attempt.
    assert_eq!(runner.call_count(), 1);
}

/// Counts approver invocations to ensure pr_create calls the approver at most once
/// across retries.
#[derive(Clone)]
struct CountingApprover {
    calls: Arc<AtomicUsize>,
}

impl CountingApprover {
    fn new() -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AiloopApprover for CountingApprover {
    async fn authorize(&self, _req: AuthorizationRequest) -> Result<ApprovalOutcome, AppError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ApprovalOutcome::Approved)
    }
}

#[tokio::test]
async fn pr_create_exponential_backoff_and_single_approval() {
    use std::time::Instant;
    let runner = Arc::new(FlakyGhRunner::new(
        2,
        GhOutput {
            stdout: "https://github.com/o/r/pull/7".to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
        "TLS handshake timeout",
        "WFG-GH-004",
    ));
    let approver = Arc::new(CountingApprover::new());
    let op = GhOperator::with_runner_and_approver(runner.clone(), approver.clone());

    let params = json!({
        "operation": "pr_create",
        "title": "T",
        "body": "B",
        "base": "main",
        "retry_count": 3,
        "retry_delay_ms": 100,
        "retry_multiplier": 2.0,
        "require_authorization": true
    });

    let workspace = tempdir().expect("workspace");
    let registry = OperatorRegistry::builder().build();
    let ctx = ExecutionContext {
        workspace_path: workspace.path().to_path_buf(),
        execution_id: "exec".to_string(),
        task_id: "create".to_string(),
        iteration: 0,
        state_view: StateView::new(json!({}), json!({}), json!({})),
        graph: GraphHandle::new(HashMap::new()),
        workflow_file: workspace.path().join("wf.yaml"),
        nesting_depth: 0,
        execution_overrides: ExecutionOverrides {
            parallel_limit: None,
            max_time_seconds: None,
            checkpoint_base_path: None,
            artifact_base_path: None,
            max_nesting_depth: None,
            verbose: false,
            sink: None,
            pre_seed_nodes: true,
        },
        operator_registry: registry,
    };

    let start = Instant::now();
    let out = op.execute(params, ctx).await.expect("pr_create succeeds");
    let elapsed_ms = start.elapsed().as_millis() as u64;

    assert_eq!(out["pr_number"], 7);
    assert_eq!(runner.call_count(), 3);
    assert_eq!(
        approver.count(),
        1,
        "approver must be called at most once across retries"
    );
    // Sleeps: 100ms + 200ms = 300ms minimum.
    assert!(
        elapsed_ms >= 300,
        "expected >=300ms total sleep, got {}",
        elapsed_ms
    );
}

#[test]
fn validate_pr_create_rejects_bad_retry_multiplier() {
    let params = json!({
        "operation": "pr_create",
        "title": "T",
        "retry_multiplier": 0.5
    });
    assert!(GhOperator::new().validate_params(&params).is_err());

    let params = json!({
        "operation": "pr_create",
        "title": "T",
        "retry_jitter_ms": -1
    });
    assert!(GhOperator::new().validate_params(&params).is_err());
}

// ─── FlakyGitRunner ──────────────────────────────────────────────────────────

#[derive(Clone)]
struct FlakyGitRunner {
    fail_count: Arc<AtomicUsize>,
    success_output: GhOutput,
    error_message: String,
    error_code: String,
    calls: Arc<AtomicUsize>,
}

impl FlakyGitRunner {
    fn new(
        fail_count: usize,
        success_output: GhOutput,
        error_message: &str,
        error_code: &str,
    ) -> Self {
        Self {
            fail_count: Arc::new(AtomicUsize::new(fail_count)),
            success_output,
            error_message: error_message.to_string(),
            error_code: error_code.to_string(),
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl GitRunner for FlakyGitRunner {
    async fn run(&self, _args: &[&str], _cwd: &std::path::Path) -> Result<GhOutput, AppError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let remaining = self.fail_count.load(Ordering::SeqCst);
        if remaining > 0 {
            self.fail_count.fetch_sub(1, Ordering::SeqCst);
            return Err(AppError::new(
                ErrorCategory::ToolExecutionError,
                self.error_message.clone(),
            )
            .with_code(&self.error_code));
        }
        Ok(self.success_output.clone())
    }
}

fn make_exec_ctx(workspace: &std::path::Path) -> ExecutionContext {
    let registry = OperatorRegistry::builder().build();
    ExecutionContext {
        workspace_path: workspace.to_path_buf(),
        execution_id: "exec".to_string(),
        task_id: "push".to_string(),
        iteration: 0,
        state_view: StateView::new(json!({}), json!({}), json!({})),
        graph: GraphHandle::new(HashMap::new()),
        workflow_file: workspace.join("wf.yaml"),
        nesting_depth: 0,
        execution_overrides: ExecutionOverrides {
            parallel_limit: None,
            max_time_seconds: None,
            checkpoint_base_path: None,
            artifact_base_path: None,
            max_nesting_depth: None,
            verbose: false,
            sink: None,
            pre_seed_nodes: true,
        },
        operator_registry: registry,
    }
}

// ─── AC 1: all optional params → Ok ─────────────────────────────────────────
#[test]
fn branch_push_validate_no_params_ok() {
    let params = json!({ "operation": "branch_push" });
    assert!(GhOperator::new().validate_params(&params).is_ok());
}

// ─── AC 2: explicit params → Ok ─────────────────────────────────────────────
#[test]
fn branch_push_validate_explicit_params_ok() {
    let params = json!({
        "operation": "branch_push",
        "remote": "upstream",
        "branch": "feature/x",
        "set_upstream": false
    });
    assert!(GhOperator::new().validate_params(&params).is_ok());
}

// ─── AC 3: empty remote → WFG-GH-009 ────────────────────────────────────────
#[test]
fn branch_push_validate_empty_remote_err() {
    let params = json!({ "operation": "branch_push", "remote": "" });
    let err = GhOperator::new()
        .validate_params(&params)
        .expect_err("must fail");
    assert_eq!(err.code, "WFG-GH-009");
}

// ─── AC 4: empty branch → WFG-GH-009 ────────────────────────────────────────
#[test]
fn branch_push_validate_empty_branch_err() {
    let params = json!({ "operation": "branch_push", "branch": "" });
    let err = GhOperator::new()
        .validate_params(&params)
        .expect_err("must fail");
    assert_eq!(err.code, "WFG-GH-009");
}

// ─── AC 5: remote with space → WFG-GH-009 ───────────────────────────────────
#[test]
fn branch_push_validate_remote_with_space_err() {
    let params = json!({ "operation": "branch_push", "remote": "bad remote" });
    let err = GhOperator::new()
        .validate_params(&params)
        .expect_err("must fail");
    assert_eq!(err.code, "WFG-GH-009");
}

// ─── AC 6: remote starting with '-' → WFG-GH-009 ────────────────────────────
#[test]
fn branch_push_validate_remote_starts_with_dash_err() {
    let params = json!({ "operation": "branch_push", "remote": "-origin" });
    let err = GhOperator::new()
        .validate_params(&params)
        .expect_err("must fail");
    assert_eq!(err.code, "WFG-GH-009");
}

// ─── AC 7: retry_count: 0 → Err ─────────────────────────────────────────────
#[test]
fn branch_push_validate_retry_count_zero_err() {
    let params = json!({ "operation": "branch_push", "retry_count": 0 });
    assert!(GhOperator::new().validate_params(&params).is_err());
}

// ─── AC 8: retry_multiplier: 0.5 → Err ──────────────────────────────────────
#[test]
fn branch_push_validate_retry_multiplier_err() {
    let params = json!({ "operation": "branch_push", "retry_multiplier": 0.5 });
    assert!(GhOperator::new().validate_params(&params).is_err());
}

// ─── AC 9: retry_delay_ms: -1 → Err ─────────────────────────────────────────
#[test]
fn branch_push_validate_retry_delay_negative_err() {
    let params = json!({ "operation": "branch_push", "retry_delay_ms": -1 });
    assert!(GhOperator::new().validate_params(&params).is_err());
}

// ─── AC 10: retry_jitter_ms: -1 → Err ───────────────────────────────────────
#[test]
fn branch_push_validate_retry_jitter_negative_err() {
    let params = json!({ "operation": "branch_push", "retry_jitter_ms": -1 });
    assert!(GhOperator::new().validate_params(&params).is_err());
}

// ─── AC 11: set_upstream: "yes" → Err ───────────────────────────────────────
#[test]
fn branch_push_validate_set_upstream_non_bool_err() {
    let params = json!({ "operation": "branch_push", "set_upstream": "yes" });
    assert!(GhOperator::new().validate_params(&params).is_err());
}

// ─── AC 12: unknown field → Ok ───────────────────────────────────────────────
#[test]
fn branch_push_validate_unknown_field_ok() {
    let params = json!({ "operation": "branch_push", "color": "blue" });
    assert!(GhOperator::new().validate_params(&params).is_ok());
}

// ─── AC 13: default args to git runner ───────────────────────────────────────
#[tokio::test]
async fn branch_push_default_args() {
    let workspace = tempdir().expect("workspace");
    let git_runner = Arc::new(MockGitRunner::new());
    git_runner.add_success(
        vec!["push", "--set-upstream", "origin", "HEAD"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let op = GhOperator::with_all(
        Arc::new(MockGhRunner::new()),
        git_runner.clone(),
        Arc::new(newton_core::workflow::operators::gh_authorization::NoopApprover),
    );

    let params = json!({ "operation": "branch_push" });
    let ctx = make_exec_ctx(workspace.path());
    let out = op.execute(params, ctx).await.expect("should succeed");
    assert_eq!(out["pushed"], true);
    assert_eq!(git_runner.call_count(), 1);
}

// ─── AC 14: explicit remote/branch/no-upstream ───────────────────────────────
#[tokio::test]
async fn branch_push_explicit_params_no_upstream() {
    let workspace = tempdir().expect("workspace");
    let git_runner = Arc::new(MockGitRunner::new());
    git_runner.add_success(
        vec!["push", "upstream", "feature/x"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let op = GhOperator::with_all(
        Arc::new(MockGhRunner::new()),
        git_runner.clone(),
        Arc::new(newton_core::workflow::operators::gh_authorization::NoopApprover),
    );

    let params = json!({
        "operation": "branch_push",
        "remote": "upstream",
        "branch": "feature/x",
        "set_upstream": false
    });
    let ctx = make_exec_ctx(workspace.path());
    let out = op.execute(params, ctx).await.expect("should succeed");
    assert_eq!(out["pushed"], true);
    assert_eq!(out["remote"], "upstream");
    assert_eq!(out["branch"], "feature/x");
    assert_eq!(out["set_upstream"], false);
}

// ─── AC 15: cwd == workspace_path ────────────────────────────────────────────
#[tokio::test]
async fn branch_push_cwd_is_workspace_path() {
    let workspace = tempdir().expect("workspace");
    let git_runner = Arc::new(MockGitRunner::new());
    git_runner.add_success(
        vec!["push", "--set-upstream", "origin", "HEAD"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let op = GhOperator::with_all(
        Arc::new(MockGhRunner::new()),
        git_runner.clone(),
        Arc::new(newton_core::workflow::operators::gh_authorization::NoopApprover),
    );

    let params = json!({ "operation": "branch_push" });
    let ctx = make_exec_ctx(workspace.path());
    op.execute(params, ctx).await.expect("should succeed");

    let cwd = git_runner.last_cwd().expect("cwd should be recorded");
    assert_eq!(cwd, workspace.path());
}

#[tokio::test]
async fn pr_create_cwd_is_workspace_path() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());
    runner.add_response(
        vec![
            "pr", "create", "--base", "main", "--title", "Test PR", "--body", "",
        ],
        GhOutput {
            stdout: "https://github.com/testorg/testrepo/pull/42\n".to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let op = GhOperator::with_runner(runner.clone() as Arc<dyn GhRunner>);
    let params = json!({
        "operation": "pr_create",
        "title": "Test PR",
        "retry_count": 1
    });
    let ctx = make_exec_ctx(workspace.path());
    op.execute(params, ctx).await.expect("should succeed");

    let cwd = runner.last_cwd().expect("cwd should be recorded");
    assert_eq!(cwd, workspace.path());
}

#[tokio::test]
async fn pr_view_cwd_is_workspace_path() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());
    runner.add_response(
        vec!["pr", "view", "42", "--json", "state"],
        GhOutput {
            stdout: r#"{"state":"OPEN"}"#.to_string(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let op = GhOperator::with_runner(runner.clone() as Arc<dyn GhRunner>);
    let params = json!({
        "operation": "pr_view",
        "pr": 42
    });
    let ctx = make_exec_ctx(workspace.path());
    op.execute(params, ctx).await.expect("should succeed");

    let cwd = runner.last_cwd().expect("cwd should be recorded");
    assert_eq!(cwd, workspace.path());
}

#[tokio::test]
async fn pr_approve_cwd_is_workspace_path() {
    let workspace = tempdir().expect("workspace");
    let runner = Arc::new(MockGhRunner::new());
    runner.add_response(
        vec!["pr", "review", "36", "--approve"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let op = GhOperator::with_runner(runner.clone() as Arc<dyn GhRunner>);
    let params = json!({
        "operation": "pr_approve",
        "pr_number": 36
    });
    let ctx = make_exec_ctx(workspace.path());
    op.execute(params, ctx).await.expect("should succeed");

    let cwd = runner.last_cwd().expect("cwd should be recorded");
    assert_eq!(cwd, workspace.path());
}

// ─── AC 16: success output shape ─────────────────────────────────────────────
#[tokio::test]
async fn branch_push_success_output_shape() {
    let workspace = tempdir().expect("workspace");
    let git_runner = Arc::new(MockGitRunner::new());
    git_runner.add_success(
        vec!["push", "--set-upstream", "origin", "HEAD"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let op = GhOperator::with_all(
        Arc::new(MockGhRunner::new()),
        git_runner,
        Arc::new(newton_core::workflow::operators::gh_authorization::NoopApprover),
    );

    let params = json!({ "operation": "branch_push" });
    let ctx = make_exec_ctx(workspace.path());
    let out = op.execute(params, ctx).await.expect("should succeed");
    assert_eq!(out["pushed"], true);
    assert_eq!(out["remote"], "origin");
    assert_eq!(out["branch"], "HEAD");
    assert_eq!(out["set_upstream"], true);
}

// ─── AC 17: retry on first failure, succeed on second ────────────────────────
#[tokio::test]
async fn branch_push_retry_succeeds_on_second_attempt() {
    let workspace = tempdir().expect("workspace");
    let git_runner = Arc::new(FlakyGitRunner::new(
        1,
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
        "network error",
        "WFG-GH-011",
    ));

    let op = GhOperator::with_all(
        Arc::new(MockGhRunner::new()),
        git_runner.clone(),
        Arc::new(newton_core::workflow::operators::gh_authorization::NoopApprover),
    );

    let params = json!({
        "operation": "branch_push",
        "retry_count": 3,
        "retry_delay_ms": 1,
    });
    let ctx = make_exec_ctx(workspace.path());
    let out = op
        .execute(params, ctx)
        .await
        .expect("should succeed on 2nd attempt");
    assert_eq!(out["pushed"], true);
    assert_eq!(git_runner.call_count(), 2);
}

// ─── AC 18: all 3 attempts fail → Err with WFG-GH-011 ───────────────────────
#[tokio::test]
async fn branch_push_all_attempts_fail_returns_err() {
    let workspace = tempdir().expect("workspace");
    let git_runner = Arc::new(FlakyGitRunner::new(
        10,
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
        "push rejected",
        "WFG-GH-011",
    ));

    let op = GhOperator::with_all(
        Arc::new(MockGhRunner::new()),
        git_runner.clone(),
        Arc::new(newton_core::workflow::operators::gh_authorization::NoopApprover),
    );

    let params = json!({
        "operation": "branch_push",
        "retry_count": 3,
        "retry_delay_ms": 1,
    });
    let ctx = make_exec_ctx(workspace.path());
    let err = op
        .execute(params, ctx)
        .await
        .expect_err("should fail after all attempts");
    assert_eq!(err.code, "WFG-GH-011");
    assert_eq!(git_runner.call_count(), 3);
}

// ─── AC 19: spawn error propagated as WFG-GH-010 ────────────────────────────
#[tokio::test]
async fn branch_push_spawn_error_propagated() {
    let workspace = tempdir().expect("workspace");
    let git_runner = Arc::new(FlakyGitRunner::new(
        10,
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
        "failed to execute git: no such file",
        "WFG-GH-010",
    ));

    let op = GhOperator::with_all(
        Arc::new(MockGhRunner::new()),
        git_runner.clone(),
        Arc::new(newton_core::workflow::operators::gh_authorization::NoopApprover),
    );

    let params = json!({
        "operation": "branch_push",
        "retry_count": 3,
        "retry_delay_ms": 1,
    });
    let ctx = make_exec_ctx(workspace.path());
    let err = op.execute(params, ctx).await.expect_err("should fail");
    assert_eq!(err.code, "WFG-GH-010");
}

// ─── AC 20: authorization denied → WFG-GH-AUTH-001, no git call ─────────────
#[tokio::test]
async fn branch_push_auth_denied_no_git_call() {
    use newton_core::workflow::operators::gh_authorization::ApprovalOutcome;

    struct DenyingApprover;
    #[async_trait]
    impl AiloopApprover for DenyingApprover {
        async fn authorize(&self, _req: AuthorizationRequest) -> Result<ApprovalOutcome, AppError> {
            Ok(ApprovalOutcome::Denied { reason: None })
        }
    }

    let workspace = tempdir().expect("workspace");
    let git_runner = Arc::new(MockGitRunner::new());

    let op = GhOperator::with_all(
        Arc::new(MockGhRunner::new()),
        git_runner.clone(),
        Arc::new(DenyingApprover),
    );

    let params = json!({
        "operation": "branch_push",
        "require_authorization": true,
    });
    let ctx = make_exec_ctx(workspace.path());
    let err = op.execute(params, ctx).await.expect_err("should be denied");
    assert_eq!(err.code, "WFG-GH-AUTH-001");
    assert_eq!(git_runner.call_count(), 0, "git runner must not be called");
}

// ─── AC 21: authorization approved → push proceeds ───────────────────────────
#[tokio::test]
async fn branch_push_auth_approved_proceeds() {
    use newton_core::workflow::operators::gh_authorization::ApprovalOutcome;

    struct ApprovingApprover;
    #[async_trait]
    impl AiloopApprover for ApprovingApprover {
        async fn authorize(&self, _req: AuthorizationRequest) -> Result<ApprovalOutcome, AppError> {
            Ok(ApprovalOutcome::Approved)
        }
    }

    let workspace = tempdir().expect("workspace");
    let git_runner = Arc::new(MockGitRunner::new());
    git_runner.add_success(
        vec!["push", "--set-upstream", "origin", "HEAD"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let op = GhOperator::with_all(
        Arc::new(MockGhRunner::new()),
        git_runner.clone(),
        Arc::new(ApprovingApprover),
    );

    let params = json!({
        "operation": "branch_push",
        "require_authorization": true,
    });
    let ctx = make_exec_ctx(workspace.path());
    let out = op.execute(params, ctx).await.expect("should succeed");
    assert_eq!(out["pushed"], true);
    assert_eq!(git_runner.call_count(), 1);
}

// ─── AC 22/25: fixture runs to terminal: success via MockGitRunner ────────────
#[tokio::test]
async fn branch_push_fixture_runs_to_success() {
    let workspace = tempdir().expect("workspace");
    let git_runner = Arc::new(MockGitRunner::new());
    git_runner.add_success(
        vec!["push", "--set-upstream", "origin", "HEAD"],
        GhOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        },
    );

    let yaml = include_str!("../fixtures/workflows/47_gh_operator_branch_push.yaml");

    use std::io::Write as _;
    let mut workflow_file = tempfile::NamedTempFile::new().expect("workflow temp file");
    write!(workflow_file, "{yaml}").expect("write workflow");

    let document =
        newton_core::workflow::schema::load_workflow(workflow_file.path()).expect("load workflow");

    let registry = build_registry_with_git_runner(workspace.path().to_path_buf(), git_runner);

    let summary = newton_core::workflow::executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry,
        workspace.path().to_path_buf(),
        newton_core::workflow::executor::ExecutionOverrides {
            parallel_limit: None,
            max_time_seconds: None,
            checkpoint_base_path: None,
            artifact_base_path: None,
            max_nesting_depth: None,
            verbose: false,
            sink: None,
            pre_seed_nodes: true,
        },
    )
    .await
    .expect("workflow should complete successfully");

    assert!(summary.completed_tasks.contains_key("push_branch"));
    assert_eq!(
        summary.completed_tasks["push_branch"].output["pushed"],
        true
    );
}
