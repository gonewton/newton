//! End-to-end tests for `HumanDecisionOperator` and `HumanApprovalOperator`
//! routed through the ailoop HTTP transport via `AiloopInterviewer`.
//!
//! These tests exercise the new transport with a `wiremock` server stub and
//! assert that the operator output JSON shapes and audit log entries remain
//! identical to the console backend (audit `interviewer_type` is the only
//! differing field).

use insta::assert_yaml_snapshot;
use newton_core::integrations::ailoop::config::AiloopConfig;
use newton_core::integrations::ailoop::tool_client::ToolClient;
use newton_core::integrations::ailoop::AiloopContext;
use newton_core::workflow::executor::{ExecutionOverrides, GraphHandle};
use newton_core::workflow::human::AiloopInterviewer;
use newton_core::workflow::operator::{ExecutionContext, Operator, OperatorRegistry, StateView};
use newton_core::workflow::operators::{
    human_approval::HumanApprovalOperator, human_decision::HumanDecisionOperator,
};
use newton_core::workflow::schema::HumanSettings;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use url::Url;
use uuid::Uuid;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn build_ailoop_interviewer(server_uri: &str, channel: &str) -> Arc<AiloopInterviewer> {
    let config = AiloopConfig {
        http_url: Url::parse(server_uri).unwrap(),
        ws_url: Url::parse("ws://127.0.0.1:1").unwrap(),
        channel: channel.to_string(),
        enabled: true,
        fail_fast: false,
    };
    let ctx = Arc::new(AiloopContext::new(
        config,
        PathBuf::from("/tmp"),
        "test".to_string(),
    ));
    let client = Arc::new(ToolClient::new(ctx));
    Arc::new(AiloopInterviewer::new(
        client,
        false,
        Duration::from_secs(60),
    ))
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

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(format!(r"^/+questions/{channel}$")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "answer": "fix",
            "timed_out": false,
        })))
        .mount(&server)
        .await;

    let interviewer = build_ailoop_interviewer(&server.uri(), channel);
    let operator =
        HumanDecisionOperator::new(interviewer, HumanSettings::default(), Arc::new(Vec::new()));
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

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(format!(r"^/+authorization/{channel}$")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "authorized": false,
            "timed_out": true,
            "reason": null,
        })))
        .mount(&server)
        .await;

    let interviewer = build_ailoop_interviewer(&server.uri(), channel);
    let operator =
        HumanApprovalOperator::new(interviewer, HumanSettings::default(), Arc::new(Vec::new()));
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
