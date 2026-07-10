/// Spec 074 B14 — Workflow/logs WS: `tokio::select!` over recv / socket read
/// half / 30s ping.
///
/// Before this fix, `handle_workflow_socket` and `handle_logs_socket` only
/// ever `match rx.recv().await { ... }`: they never read from the client's
/// half of the socket. That meant (a) no periodic ping keepalive on these
/// two endpoints (unlike `/ws`, which already selects over a ping tick), and
/// (b) a client-initiated WebSocket Close frame was never observed
/// server-side — the handler task only noticed disconnection on its next
/// `socket.send(...)`, which for an instance/node with no further broadcast
/// traffic could never happen, leaving the task (and its broadcast
/// subscription) alive forever.
///
/// These tests assert both halves of the fix:
///   1. Ping cadence: with `AppState::with_ws_ping_interval` shrunk to a few
///      milliseconds, the server sends a `Message::Ping` within a bounded,
///      short wait on both endpoints.
///   2. Close detection: after the client sends a WS Close frame, the
///      server-side handler task exits promptly, observed via
///      `broadcast::Sender::receiver_count()` dropping to 0 (proof the task
///      dropped its subscription and returned, not just that the client's
///      own socket closed).
use futures::{SinkExt, StreamExt};
use newton_backend::BackendStore;
use newton_core::api::state::AppState;
use newton_types::{OperatorDescriptor, WorkflowInstance, WorkflowStatus};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

async fn make_backend() -> Arc<dyn BackendStore> {
    let store = newton_backend::SqliteBackendStore::new_in_memory()
        .await
        .expect("in-memory backend init");
    Arc::new(store)
}

fn make_state(backend: Arc<dyn BackendStore>, ping_interval: Duration) -> AppState {
    let operators = vec![OperatorDescriptor {
        operator_type: "noop".to_string(),
        description: "No-op".to_string(),
        params_schema: json!({}),
    }];
    AppState::new(operators, backend).with_ws_ping_interval(ping_interval)
}

async fn spawn_router(app: axum::Router) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    port
}

async fn insert_instance(backend: &Arc<dyn BackendStore>, instance_id: &str) {
    backend
        .upsert_workflow_instance(&WorkflowInstance {
            instance_id: instance_id.to_string(),
            workflow_id: "wf-select".to_string(),
            status: WorkflowStatus::Running,
            nodes: vec![],
            started_at: chrono::Utc::now(),
            ended_at: None,
            definition: None,
            linked_plan_id: None,
        })
        .await
        .unwrap();
}

/// Test 1: `/stream/workflow/{id}/ws` sends a `Ping` frame within a bounded,
/// short wait when the ping interval is shrunk — proof the select loop's
/// ping-tick branch fires on an otherwise-idle connection (no broadcast
/// traffic at all).
#[tokio::test]
async fn workflow_ws_sends_ping_on_idle_connection() {
    let backend = make_backend().await;
    let instance_id = Uuid::new_v4().to_string();
    insert_instance(&backend, &instance_id).await;

    let state = make_state(Arc::clone(&backend), Duration::from_millis(50));
    let app = newton_core::api::api_v1_router(state);
    let port = spawn_router(app).await;

    let ws_url = format!("ws://127.0.0.1:{port}/stream/workflow/{instance_id}/ws");
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect");

    // Drain the connect-time snapshot frame first.
    let snapshot = ws_stream.next().await.unwrap().unwrap();
    assert!(matches!(snapshot, WsMessage::Text(_)));

    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ws_stream.next().await.unwrap().unwrap() {
                WsMessage::Ping(_) => return,
                _ => continue,
            }
        }
    })
    .await;

    assert!(
        result.is_ok(),
        "expected a Ping frame on an idle workflow WS connection within the timeout"
    );
}

/// Same ping-cadence contract for `/stream/logs/{id}/{node_id}/ws`.
#[tokio::test]
async fn logs_ws_sends_ping_on_idle_connection() {
    let backend = make_backend().await;
    let instance_id = Uuid::new_v4().to_string();
    let node_id = "ping-task";
    insert_instance(&backend, &instance_id).await;

    let state = make_state(Arc::clone(&backend), Duration::from_millis(50));
    let app = newton_core::api::api_v1_router(state);
    let port = spawn_router(app).await;

    let ws_url = format!("ws://127.0.0.1:{port}/stream/logs/{instance_id}/{node_id}/ws");
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect");

    // Drain the "Connected to ..." connect-line frame first.
    let connect_frame = ws_stream.next().await.unwrap().unwrap();
    assert!(matches!(connect_frame, WsMessage::Text(_)));

    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ws_stream.next().await.unwrap().unwrap() {
                WsMessage::Ping(_) => return,
                _ => continue,
            }
        }
    })
    .await;

    assert!(
        result.is_ok(),
        "expected a Ping frame on an idle logs WS connection within the timeout"
    );
}

/// Test 2: `/stream/workflow/{id}/ws` — a client-sent Close frame must make
/// the server-side handler task exit promptly, proven by
/// `events_tx.receiver_count()` dropping to 0 shortly after (rather than the
/// task lingering, subscribed forever, until some unrelated future broadcast
/// send happens to fail).
#[tokio::test]
async fn workflow_ws_exits_promptly_on_client_close() {
    let backend = make_backend().await;
    let instance_id = Uuid::new_v4().to_string();
    insert_instance(&backend, &instance_id).await;

    // Ping interval deliberately left long (the default) so the only way
    // this test can observe prompt task exit is via the socket-read half of
    // the select loop noticing the Close frame — not a ping-driven send
    // failure.
    let state = make_state(Arc::clone(&backend), Duration::from_secs(30));
    let events_tx = state.events_tx.clone();
    let app = newton_core::api::api_v1_router(state);
    let port = spawn_router(app).await;

    let ws_url = format!("ws://127.0.0.1:{port}/stream/workflow/{instance_id}/ws");
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect");

    let snapshot = ws_stream.next().await.unwrap().unwrap();
    assert!(matches!(snapshot, WsMessage::Text(_)));

    // The handler's subscription is the only receiver in this test.
    assert_eq!(events_tx.receiver_count(), 1);

    ws_stream
        .send(WsMessage::Close(None))
        .await
        .expect("send client Close frame");

    let result = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if events_tx.receiver_count() == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;

    assert!(
        result.is_ok(),
        "expected the workflow WS handler task to drop its broadcast \
         subscription promptly after a client Close frame"
    );
}

/// Same client-Close contract for `/stream/logs/{id}/{node_id}/ws`.
#[tokio::test]
async fn logs_ws_exits_promptly_on_client_close() {
    let backend = make_backend().await;
    let instance_id = Uuid::new_v4().to_string();
    let node_id = "close-task";
    insert_instance(&backend, &instance_id).await;

    let state = make_state(Arc::clone(&backend), Duration::from_secs(30));
    let events_tx = state.events_tx.clone();
    let app = newton_core::api::api_v1_router(state);
    let port = spawn_router(app).await;

    let ws_url = format!("ws://127.0.0.1:{port}/stream/logs/{instance_id}/{node_id}/ws");
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect");

    let connect_frame = ws_stream.next().await.unwrap().unwrap();
    assert!(matches!(connect_frame, WsMessage::Text(_)));

    assert_eq!(events_tx.receiver_count(), 1);

    ws_stream
        .send(WsMessage::Close(None))
        .await
        .expect("send client Close frame");

    let result = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if events_tx.receiver_count() == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;

    assert!(
        result.is_ok(),
        "expected the logs WS handler task to drop its broadcast \
         subscription promptly after a client Close frame"
    );
}
