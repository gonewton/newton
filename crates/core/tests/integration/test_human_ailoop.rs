//! End-to-end tests for `HumanDecisionOperator` and `HumanApprovalOperator`
//! routed through the ailoop WebSocket transport via `AiloopInterviewer`.
//!
//! A minimal in-process WS server responds with the expected `MessageContent`
//! variant. This exercises the full operator→interviewer→ailoop-core path.

use ailoop_core::models::{Message, MessageContent, ResponseType};
use futures::{SinkExt, StreamExt};
use insta::assert_yaml_snapshot;
use newton_core::workflow::executor::{ExecutionOverrides, GraphHandle};
use newton_core::workflow::human::{AiloopInterviewer, Interviewer, InterviewerProvider};
use newton_core::workflow::operator::{ExecutionContext, Operator, OperatorRegistry, StateView};
use newton_core::workflow::operators::{
    human_approval::HumanApprovalOperator, human_decision::HumanDecisionOperator,
};
use newton_core::workflow::schema::HumanSettings;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

/// Start a minimal WS server that responds once with `response_content`.
/// Returns the ws:// URL and a JoinHandle.
async fn start_ws_responder(
    response_content: MessageContent,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("ws://127.0.0.1:{port}");

    let handle = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> =
                tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut sender, mut receiver) = ws.split();

            if let Some(Ok(WsMessage::Text(text))) = receiver.next().await {
                let msg: Message = serde_json::from_str(&text).unwrap();
                let reply = Message::response(msg.channel.clone(), response_content, msg.id);
                let reply_json = serde_json::to_string(&reply).unwrap();
                let _ = sender.send(WsMessage::Text(reply_json)).await;
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(5)).await;
    (url, handle)
}

fn build_ailoop_interviewer(ws_url: &str, channel: &str) -> Arc<AiloopInterviewer> {
    Arc::new(AiloopInterviewer::new(
        ws_url.to_string(),
        channel.to_string(),
        false,
        Duration::from_secs(60),
    ))
}

fn provider_for(interviewer: Arc<AiloopInterviewer>) -> InterviewerProvider {
    let cloned: Arc<dyn Interviewer> = interviewer;
    Arc::new(move || Ok(cloned.clone()))
}

fn build_execution_context(workspace: &TempDir, execution_id: String) -> ExecutionContext {
    let empty = Value::Object(Map::new());
    ExecutionContext {
        workspace_path: workspace.path().to_path_buf(),
        execution_id,
        task_id: "task".to_string(),
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

fn redact_audit(entry: &mut Value) {
    if let Some(obj) = entry.as_object_mut() {
        if obj.contains_key("execution_id") {
            obj.insert("execution_id".to_string(), json!("[execution_id]"));
        }
        if obj.contains_key("timestamp") {
            obj.insert("timestamp".to_string(), json!("[timestamp]"));
        }
    }
}

#[tokio::test]
async fn human_decision_via_ailoop_happy_path() {
    let workspace = TempDir::new().unwrap();
    let execution_id = Uuid::new_v4().to_string();
    let channel = "decision-test";

    let (ws_url, _handle) = start_ws_responder(MessageContent::Response {
        response_type: ResponseType::Text,
        answer: Some("fix".to_string()),
    })
    .await;

    let interviewer = build_ailoop_interviewer(&ws_url, channel);
    let operator = HumanDecisionOperator::new(
        provider_for(interviewer),
        HumanSettings::default(),
        Arc::new(Vec::new()),
    );
    let mut ctx = build_execution_context(&workspace, execution_id.clone());
    ctx.task_id = "decision".to_string();

    let output = operator
        .execute(
            json!({
                "prompt": "Which path forward?",
                "choices": ["fix", "skip", "abort"],
                "timeout_seconds": 60,
                "default_choice": "skip",
            }),
            ctx,
        )
        .await
        .expect("execute should succeed");

    // Goal 7: operator output JSON shape unchanged
    assert_eq!(output["choice"], json!("fix"));
    assert!(output.get("timestamp").is_some());

    // Goal 6: audit entry has interviewer_type=ailoop
    let audit_path = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(&execution_id)
        .join("audit.jsonl");
    let contents = std::fs::read_to_string(&audit_path).expect("audit file written");
    let line = contents.lines().next().expect("at least one audit entry");
    let mut entry: Value = serde_json::from_str(line).expect("audit entry is JSON");
    assert_eq!(entry["interviewer_type"], json!("ailoop"));
    assert_eq!(entry["choice"], json!("fix"));
    assert_eq!(entry["task_id"], json!("decision"));

    redact_audit(&mut entry);
    assert_yaml_snapshot!("human_decision_via_ailoop", entry);
}

#[tokio::test]
async fn human_approval_via_ailoop_timeout_default() {
    let workspace = TempDir::new().unwrap();
    let execution_id = Uuid::new_v4().to_string();
    let channel = "approval-test";

    let (ws_url, _handle) = start_ws_responder(MessageContent::Response {
        response_type: ResponseType::Timeout,
        answer: None,
    })
    .await;

    let interviewer = build_ailoop_interviewer(&ws_url, channel);
    let operator = HumanApprovalOperator::new(
        provider_for(interviewer),
        HumanSettings::default(),
        Arc::new(Vec::new()),
    );
    let mut ctx = build_execution_context(&workspace, execution_id.clone());
    ctx.task_id = "approval".to_string();

    let output = operator
        .execute(
            json!({
                "prompt": "Approve release?",
                "timeout_seconds": 1,
                "default_on_timeout": "approve",
            }),
            ctx,
        )
        .await
        .expect("execute should succeed");

    // Goal 7: operator output JSON shape unchanged
    assert_eq!(output["approved"], json!(true));
    assert!(output.get("reason").is_some());
    assert!(output.get("timestamp").is_some());

    // Goal 6: audit entry has interviewer_type=ailoop and reflects timeout-applied default
    let audit_path = workspace
        .path()
        .join(".newton")
        .join("state")
        .join("workflows")
        .join(&execution_id)
        .join("audit.jsonl");
    let contents = std::fs::read_to_string(&audit_path).expect("audit file written");
    let line = contents.lines().next().expect("at least one audit entry");
    let mut entry: Value = serde_json::from_str(line).expect("audit entry is JSON");
    assert_eq!(entry["interviewer_type"], json!("ailoop"));
    assert_eq!(entry["approved"], json!(true));
    assert_eq!(entry["timeout_applied"], json!(true));
    assert_eq!(entry["default_used"], json!(true));

    redact_audit(&mut entry);
    assert_yaml_snapshot!("human_approval_via_ailoop_timeout_default", entry);
}

#[tokio::test]
async fn human_decision_without_ailoop_emits_hil_001() {
    let workspace = TempDir::new().unwrap();
    let execution_id = Uuid::new_v4().to_string();
    let provider: InterviewerProvider =
        newton_core::workflow::human::lazy_interviewer_provider(None, Duration::from_secs(60));
    let operator =
        HumanDecisionOperator::new(provider, HumanSettings::default(), Arc::new(Vec::new()));
    let mut ctx = build_execution_context(&workspace, execution_id);
    ctx.task_id = "decision".to_string();
    let err = operator
        .execute(
            json!({
                "prompt": "Pick",
                "choices": ["a", "b"],
                "timeout_seconds": 1,
                "default_choice": "a",
            }),
            ctx,
        )
        .await
        .expect_err("expected HIL-AILOOP-001");
    assert_eq!(err.code, "HIL-AILOOP-001");
}

#[test]
fn require_enabled_ailoop_context_malformed_returns_hil_003() {
    use std::fs;
    let workspace = TempDir::new().unwrap();
    let configs = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs).unwrap();
    fs::write(
        configs.join("monitor.conf"),
        "ailoop_server_ws_url=not a valid url\nailoop_channel=test\n",
    )
    .unwrap();
    std::env::set_var("NEWTON_AILOOP_INTEGRATION", "1");
    std::env::remove_var("NEWTON_AILOOP_WS_URL");
    std::env::remove_var("NEWTON_AILOOP_CHANNEL");
    let result = newton_core::integrations::ailoop::config::require_enabled_ailoop_context(
        workspace.path(),
        "run",
    );
    std::env::remove_var("NEWTON_AILOOP_INTEGRATION");
    let err = result.expect_err("malformed config must error");
    assert_eq!(err.code, "HIL-AILOOP-003");
}

#[tokio::test]
async fn lazy_provider_not_invoked_when_no_human_task() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let provider: InterviewerProvider = Arc::new(move || {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        Err(newton_core::core::error::AppError::new(
            newton_core::core::types::ErrorCategory::ValidationError,
            "should not be invoked",
        )
        .with_code("HIL-AILOOP-001"))
    });
    // Build operator but never call execute — provider must not run.
    let _operator =
        HumanDecisionOperator::new(provider, HumanSettings::default(), Arc::new(Vec::new()));
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}
