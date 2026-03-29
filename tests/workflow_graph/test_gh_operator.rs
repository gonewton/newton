use async_trait::async_trait;
use newton::core::error::AppError;
use newton::core::types::ErrorCategory;
use newton::core::workflow_graph::executor::{ExecutionOverrides, ExecutionSummary};
use newton::core::workflow_graph::operator::OperatorRegistry;
use newton::core::workflow_graph::operators::gh::{GhOutput, GhRunner};
use newton::core::workflow_graph::operators::{self, BuiltinOperatorDeps};
use serde_json::json;
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

#[derive(Clone)]
struct MockGhRunner {
    responses: Arc<Mutex<HashMap<Vec<String>, GhOutput>>>,
}

impl MockGhRunner {
    fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn add_response(&self, args: Vec<&str>, output: GhOutput) {
        let key: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.responses.lock().unwrap().insert(key, output);
    }
}

#[async_trait]
impl GhRunner for MockGhRunner {
    async fn run(&self, args: &[&str]) -> Result<GhOutput, AppError> {
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
        engine_registry: None,
        gh_runner: Some(runner),
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

    let document = newton::core::workflow_graph::schema::load_workflow(workflow_file.path())
        .expect("load workflow");

    let registry = build_registry_with_gh_runner(workspace.to_path_buf(), runner);

    newton::core::workflow_graph::executor::execute_workflow(
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
