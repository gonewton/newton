use async_trait::async_trait;
use newton::core::error::AppError;
use newton::core::types::ErrorCategory;
use newton::workflow::executor::{ExecutionOverrides, ExecutionSummary, GraphHandle};
use newton::workflow::operator::{ExecutionContext, Operator, OperatorRegistry, StateView};
use newton::workflow::operators::gh::{GhOperator, GhOutput, GhRunner};
use newton::workflow::operators::gh_authorization::{
    AiloopApprover, ApprovalOutcome, AuthorizationRequest,
};
use newton::workflow::operators::{self, BuiltinOperatorDeps};
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
}

impl MockGhRunner {
    fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn add_response(&self, args: Vec<&str>, output: GhOutput) {
        let key: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.responses.lock().unwrap().insert(key, output);
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl GhRunner for MockGhRunner {
    async fn run(&self, args: &[&str]) -> Result<GhOutput, AppError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
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
        newton::workflow::schema::load_workflow(workflow_file.path()).expect("load workflow");

    let registry = build_registry_with_gh_runner(workspace.to_path_buf(), runner);

    newton::workflow::executor::execute_workflow(
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
            server_notifier: None,
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
            server_notifier: None,
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
