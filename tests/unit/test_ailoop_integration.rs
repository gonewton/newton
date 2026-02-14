use newton::ailoop_integration::bounded_queue::BoundedQueue;
use newton::ailoop_integration::config::{init_config, AiloopConfig, CommandContext};
use newton::ailoop_integration::orchestrator_notifier::OrchestratorNotifier;
use newton::ailoop_integration::output_forwarder::{OutputForwarder, StreamKind};
use newton::ailoop_integration::tool_client::{ToolClient, ToolInteractionOutcome};
use newton::ailoop_integration::workflow_emitter::{WorkflowEvent, WorkflowEventType};
use newton::ailoop_integration::TransportState;
use newton::cli::{args::MonitorArgs, Command};
use reqwest::Url;
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn workspace_with_configs(temp: &TempDir, content: &str) -> PathBuf {
    let configs = temp.path().join(".newton").join("configs");
    std::fs::create_dir_all(&configs).unwrap();
    std::fs::write(configs.join("test.conf"), content).unwrap();
    temp.path().to_path_buf()
}

fn clear_env(keys: &[&str]) {
    for key in keys {
        env::remove_var(key);
    }
}

#[test]
fn config_precedence_env_overwrites_file() {
    let temp = TempDir::new().unwrap();
    let workspace = workspace_with_configs(
        &temp,
        "ailoop_server_http_url=http://file-http\nailoop_server_ws_url=ws://file-ws\nailoop_channel=file-channel",
    );

    env::set_var("NEWTON_AILOOP_HTTP_URL", "http://env-http");
    env::set_var("NEWTON_AILOOP_WS_URL", "ws://env-ws");
    env::set_var("NEWTON_AILOOP_CHANNEL", "env-channel");

    let command = newton::cli::Command::Monitor(newton::cli::args::MonitorArgs {
        http_url: None,
        ws_url: None,
    });
    let config = init_config(&workspace, &command).unwrap().unwrap();

    assert_eq!(config.http_url.as_str(), "http://env-http");
    assert_eq!(config.ws_url.as_str(), "ws://env-ws");
    assert_eq!(config.channel, "env-channel");

    clear_env(&[
        "NEWTON_AILOOP_HTTP_URL",
        "NEWTON_AILOOP_WS_URL",
        "NEWTON_AILOOP_CHANNEL",
    ]);
}

#[test]
fn config_invalid_url_errors() {
    let temp = TempDir::new().unwrap();
    let workspace = workspace_with_configs(&temp, "");
    env::set_var("NEWTON_AILOOP_HTTP_URL", "not-a-url");
    env::set_var("NEWTON_AILOOP_WS_URL", "ws://valid");

    let command = newton::cli::Command::Monitor(newton::cli::args::MonitorArgs {
        http_url: None,
        ws_url: None,
    });
    assert!(init_config(&workspace, &command).is_err());

    clear_env(&["NEWTON_AILOOP_HTTP_URL", "NEWTON_AILOOP_WS_URL"]);
}

#[tokio::test]
async fn notifier_payload_includes_event_fields() {
    let mock_server = MockServer::start().await;
    let config = sample_config(mock_server.uri(), "ws://localhost", "test-channel");
    let state = Arc::new(TransportState::new());
    let notifier = OrchestratorNotifier::new(
        Arc::new(config),
        reqwest::Client::new(),
        state.clone(),
    );

    let event = WorkflowEvent {
        event_type: WorkflowEventType::ExecutionStarted,
        execution_id: Uuid::new_v4(),
        iteration_number: None,
        phase: Some("execution".to_string()),
        status: "running".to_string(),
        message: Some("test event".to_string()),
        progress_percent: Some(0),
        timestamp: chrono::Utc::now(),
        workspace_identifier: "workspace".to_string(),
        command_context: CommandContext {
            name: "run".to_string(),
            workspace_path: "workspace".to_string(),
            details: BTreeMap::new(),
        },
    };

    Mock::given(method("POST"))
        .and(path("/api/v1/messages"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    notifier.enqueue(event);
    sleep(Duration::from_millis(200)).await;

    let requests = mock_server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["content"]["details"]["event_type"], "execution_started");
}

#[tokio::test]
async fn output_forwarder_priority_mapping() {
    let mock_server = MockServer::start().await;
    let config = sample_config(mock_server.uri(), "ws://localhost", "test-channel");
    let state = Arc::new(TransportState::new());
    let forwarder =
        OutputForwarder::new(Arc::new(config), reqwest::Client::new(), state.clone());

    Mock::given(method("POST"))
        .and(path("/api/v1/messages"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let execution_id = Uuid::new_v4();
    forwarder.forward_line(execution_id, StreamKind::Stdout, "stdout line");
    forwarder.forward_line(execution_id, StreamKind::Stderr, "stderr line");

    sleep(Duration::from_millis(200)).await;
    let requests = mock_server.received_requests().await.unwrap();
    let priorities: Vec<String> = requests
        .iter()
        .map(|req| {
            let body: Value = serde_json::from_slice(&req.body).unwrap();
            body["content"]["priority"].as_str().unwrap().to_string()
        })
        .collect();
    assert!(priorities.contains(&"normal".to_string()));
    assert!(priorities.contains(&"high".to_string()));
}

#[test]
fn bounded_queue_drop_oldest() {
    let queue = BoundedQueue::new(2);
    queue.push(1);
    queue.push(2);
    queue.push(3);
    assert_eq!(queue.len(), 2);
    assert_eq!(queue.try_pop(), Some(2));
    assert_eq!(queue.try_pop(), Some(3));
    assert!(queue.try_pop().is_none());
}

#[tokio::test]
async fn tool_client_timeout_returns() {
    let mock_server = MockServer::start().await;
    let config = sample_config(mock_server.uri(), "ws://localhost", "test-channel");
    let client = ToolClient::new(Arc::new(config), reqwest::Client::new());

    Mock::given(method("POST"))
        .and(path("/api/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(r"{\"id\":\"123\"}".into(), "application/json"))
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/messages/123/response"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let outcome = client
        .ask_question("question", Duration::from_millis(100), &[])
        .await
        .unwrap();
    assert_eq!(outcome, ToolInteractionOutcome::Timeout);
}

fn sample_config(http: &str, ws: &str, channel: &str) -> AiloopConfig {
    let mut details = BTreeMap::new();
    details.insert("test".to_string(), "value".to_string());
    AiloopConfig {
        http_url: Url::parse(http).unwrap(),
        ws_url: Url::parse(ws).unwrap(),
        channel: channel.to_string(),
        workspace_root: PathBuf::from("/tmp"),
        workspace_identifier: "workspace".to_string(),
        command_context: CommandContext {
            name: "run".to_string(),
            workspace_path: "workspace".to_string(),
            details,
        },
        fail_fast: false,
    }
}

#[test]
fn transport_state_records_failure_once() {
    let state = TransportState::new();
    assert!(!state.has_failure());
    state.mark_failure("first failure");
    assert!(state.has_failure());
    assert_eq!(state.failure_message(), Some("first failure".to_string()));
    state.mark_failure("second failure");
    assert_eq!(state.failure_message(), Some("first failure".to_string()));
}
