use anyhow::{bail, Result};
use newton::core::error::AppError;
use newton::core::types::ErrorCategory;
use newton::core::workflow_graph::{
    executor::{self, ExecutionOverrides},
    operator::OperatorRegistry,
    operators,
    schema::{self, TriggerType, WorkflowDocument, WorkflowTrigger},
    state, webhook,
};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::{
    env,
    ffi::OsString,
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::{NamedTempFile, TempDir};
use tokio::{fs, sync::oneshot, task::JoinHandle, time::sleep};

const MANUAL_TRIGGER_WORKFLOW: &str = r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {}
  settings:
    entry_task: start
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 2
    max_workflow_iterations: 10
  tasks:
    - id: start
      operator: SetContextOperator
      params:
        patch:
          trigger_ref:
            $expr: "triggers.pr_number"
"#;

fn write_workflow(yaml: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("temp file");
    write!(file, "{}", yaml).expect("write workflow");
    file
}

fn build_registry(workspace: PathBuf, settings: state::GraphSettings) -> OperatorRegistry {
    let mut builder = OperatorRegistry::builder();
    operators::register_builtins(&mut builder, workspace, settings);
    builder.build()
}

fn webhook_workflow(max_body_bytes: usize) -> NamedTempFile {
    let yaml = format!(
        r#"
version: "2.0"
mode: workflow_graph
workflow:
  context: {{}}
  settings:
    entry_task: start
    max_time_seconds: 60
    parallel_limit: 1
    continue_on_error: false
    max_task_iterations: 1
    max_workflow_iterations: 5
    webhook:
      enabled: true
      bind: "127.0.0.1:0"
      auth_token_env: "NEWTON_WEBHOOK_TOKEN"
      max_body_bytes: {}
  tasks:
    - id: start
      operator: NoOpOperator
      params: {{}}
"#,
        max_body_bytes
    );
    write_workflow(&yaml)
}

struct AuthTokenGuard(Option<OsString>);

impl AuthTokenGuard {
    fn set(token: &str) -> Self {
        let previous = env::var_os("NEWTON_WEBHOOK_TOKEN");
        env::set_var("NEWTON_WEBHOOK_TOKEN", token);
        AuthTokenGuard(previous)
    }
}

impl Drop for AuthTokenGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.0.take() {
            env::set_var("NEWTON_WEBHOOK_TOKEN", previous);
        } else {
            env::remove_var("NEWTON_WEBHOOK_TOKEN");
        }
    }
}

async fn spawn_webhook_server(
    document: WorkflowDocument,
    workflow_path: PathBuf,
    workspace: PathBuf,
) -> Result<(SocketAddr, JoinHandle<Result<(), AppError>>)> {
    let settings = document.workflow.settings.clone();
    let mut builder = OperatorRegistry::builder();
    operators::register_builtins(&mut builder, workspace.clone(), settings.clone());
    let registry = builder.build();
    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: None,
        artifact_base_path: None,
        verbose: false,
    };
    let (addr_tx, addr_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        webhook::serve_webhook_with_ready_notifier(
            document,
            workflow_path,
            registry,
            workspace,
            overrides,
            addr_tx,
        )
        .await
    });
    let addr = addr_rx.await.map_err(|_| {
        AppError::new(
            ErrorCategory::InternalError,
            "webhook startup canceled before bind address reported",
        )
    })?;
    Ok((addr, handle))
}

async fn read_execution_json(workspace: &Path, execution_id: &str) -> Result<Value> {
    let path = workspace
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(execution_id)
        .join("execution.json");
    for _ in 0..20 {
        if path.exists() {
            let contents = fs::read_to_string(&path).await?;
            return Ok(serde_json::from_str(&contents)?);
        }
        sleep(Duration::from_millis(50)).await;
    }
    bail!("execution.json was not written for {}", execution_id);
}

#[tokio::test]
async fn manual_trigger_payload_available() -> Result<()> {
    let workflow_file = write_workflow(MANUAL_TRIGGER_WORKFLOW);
    let mut document = schema::load_workflow(workflow_file.path())?;
    let payload = json!({
        "pr_number": 123,
        "branch": "feature/manual",
    });
    document.triggers = Some(WorkflowTrigger {
        trigger_type: TriggerType::Manual,
        schema_version: "1".into(),
        payload: payload.clone(),
    });
    let workspace_dir = TempDir::new()?;
    let workspace_path = workspace_dir.path().to_path_buf();
    let registry = build_registry(workspace_path.clone(), document.workflow.settings.clone());
    let overrides = executor::ExecutionOverrides {
        parallel_limit: Some(1),
        max_time_seconds: Some(60),
        checkpoint_base_path: None,
        artifact_base_path: None,
        verbose: false,
    };
    let summary = executor::execute_workflow(
        document,
        workflow_file.path().to_path_buf(),
        registry,
        workspace_path.clone(),
        overrides,
    )
    .await?;
    let task = summary
        .completed_tasks
        .get("start")
        .expect("start task recorded");
    assert_eq!(task.output["patch"]["trigger_ref"], payload["pr_number"]);
    Ok(())
}

#[tokio::test]
async fn webhook_rejects_invalid_auth() -> Result<()> {
    let _auth = AuthTokenGuard::set("valid-token");
    let workflow_file = webhook_workflow(1024);
    let document = schema::parse_workflow(workflow_file.path())?;
    let workspace_dir = TempDir::new()?;
    let workspace_path = workspace_dir.path().to_path_buf();
    let (addr, handle) = spawn_webhook_server(
        document,
        workflow_file.path().to_path_buf(),
        workspace_path.clone(),
    )
    .await?;
    let client = reqwest::Client::new();
    let url = format!("http://{}/v1/workflow/trigger", addr);
    let trigger_body = json!({
        "trigger": {
            "type": "webhook",
            "schema_version": "1",
            "payload": {}
        }
    });
    let resp = client.post(&url).json(&trigger_body).send().await?;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: Value = resp.json().await?;
    assert_eq!(body["error"]["code"], "WFG-WEBHOOK-401");
    let resp = client
        .post(&url)
        .json(&trigger_body)
        .bearer_auth("wrong-token")
        .send()
        .await?;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: Value = resp.json().await?;
    assert_eq!(body["error"]["code"], "WFG-WEBHOOK-401");
    handle.abort();
    let _ = handle.await;
    Ok(())
}

#[tokio::test]
async fn webhook_enforces_body_limit() -> Result<()> {
    let _auth = AuthTokenGuard::set("valid-token");
    let workflow_file = webhook_workflow(32);
    let document = schema::parse_workflow(workflow_file.path())?;
    let workspace_dir = TempDir::new()?;
    let workspace_path = workspace_dir.path().to_path_buf();
    let (addr, handle) = spawn_webhook_server(
        document,
        workflow_file.path().to_path_buf(),
        workspace_path.clone(),
    )
    .await?;
    let client = reqwest::Client::new();
    let url = format!("http://{}/v1/workflow/trigger", addr);
    let payload = json!({
        "trigger": {
            "type": "webhook",
            "schema_version": "1",
            "payload": {
                "blob": "a".repeat(512)
            }
        }
    });
    let resp = client
        .post(&url)
        .json(&payload)
        .bearer_auth("valid-token")
        .send()
        .await?;
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body: Value = resp.json().await?;
    assert_eq!(body["error"]["code"], "WFG-WEBHOOK-413");
    handle.abort();
    let _ = handle.await;
    Ok(())
}

#[tokio::test]
async fn webhook_starts_execution_and_persists_state() -> Result<()> {
    let _auth = AuthTokenGuard::set("valid-token");
    let workflow_file = webhook_workflow(2048);
    let document = schema::parse_workflow(workflow_file.path())?;
    let workspace_dir = TempDir::new()?;
    let workspace_path = workspace_dir.path().to_path_buf();
    let (addr, handle) = spawn_webhook_server(
        document,
        workflow_file.path().to_path_buf(),
        workspace_path.clone(),
    )
    .await?;
    let client = reqwest::Client::new();
    let url = format!("http://{}/v1/workflow/trigger", addr);
    let payload = json!({
        "trigger": {
            "type": "webhook",
            "schema_version": "1",
            "payload": {
                "run_id": 7,
                "branch": "main"
            }
        }
    });
    let resp = client
        .post(&url)
        .json(&payload)
        .bearer_auth("valid-token")
        .send()
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await?;
    let execution_id = body["execution_id"].as_str().expect("execution_id");
    assert_eq!(body["status"], "running");
    let execution = read_execution_json(&workspace_path, execution_id).await?;
    assert_eq!(
        execution["trigger_payload"]["run_id"],
        payload["trigger"]["payload"]["run_id"]
    );
    handle.abort();
    let _ = handle.await;
    Ok(())
}
