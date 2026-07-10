/// PR-5 / B6 — lag-tolerant streams (router seam).
///
/// One test per stream endpoint (workflow WS, logs WS, workflow SSE), matching
/// the tranche-1 PRD acceptance for spec 074 (`specs/draft/074-tranche1-prd.md`
/// §PR-5): flood the shared broadcast channel past its capacity before the
/// client reads, and assert the client receives a `{"type":"lagged","skipped":N}`
/// frame (N >= 1) followed by subsequent live events, rather than the stream
/// dying silently.
use futures::StreamExt;
use newton_backend::BackendStore;
use newton_core::api::state::{AppState, BROADCAST_CAPACITY};
use newton_types::{BroadcastEvent, OperatorDescriptor, WorkflowInstance, WorkflowStatus};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

async fn make_backend() -> Arc<dyn BackendStore> {
    let store = newton_backend::SqliteBackendStore::new_in_memory()
        .await
        .expect("in-memory backend init");
    Arc::new(store)
}

fn make_state(backend: Arc<dyn BackendStore>) -> AppState {
    let operators = vec![OperatorDescriptor {
        operator_type: "noop".to_string(),
        description: "No-op".to_string(),
        params_schema: json!({}),
    }];
    AppState::new(operators, backend)
}

/// Spawn the router on an ephemeral loopback port; returns the port.
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

/// Flood `events_tx` with `n` events that do NOT match `target_instance_id`, so
/// they overflow the channel's ring buffer without producing any forwarded
/// frames to a WS/SSE client filtered on `target_instance_id`. This is a tight,
/// non-yielding loop (no `.await`), so under the current-thread test runtime
/// the connection-handling task cannot interleave and drain any of it first —
/// the overflow is deterministic.
fn flood_noise(events_tx: &broadcast::Sender<BroadcastEvent>, n: usize) {
    for _ in 0..n {
        let _ = events_tx.send(BroadcastEvent::WorkflowInstanceUpdated {
            instance_id: "flood-noise-instance".to_string(),
        });
    }
}

async fn insert_instance(backend: &Arc<dyn BackendStore>, instance_id: &str) {
    backend
        .upsert_workflow_instance(&WorkflowInstance {
            instance_id: instance_id.to_string(),
            workflow_id: "wf-lag".to_string(),
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

/// Test 1: `/stream/workflow/{id}/ws` — flood past capacity, then assert a
/// `lagged` frame arrives followed by a live marker event.
#[tokio::test]
async fn test_workflow_ws_delivers_lagged_frame_then_resumes() {
    let backend = make_backend().await;
    let instance_id = Uuid::new_v4().to_string();
    insert_instance(&backend, &instance_id).await;

    let state = make_state(Arc::clone(&backend));
    let events_tx = state.events_tx.clone();
    let app = newton_core::api::api_v1_router(state);
    let port = spawn_router(app).await;

    let ws_url = format!("ws://127.0.0.1:{port}/stream/workflow/{instance_id}/ws");
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect");

    // First frame is the connect-time `workflowInstanceUpdated` snapshot. Its
    // arrival proves the handler has already subscribed to events_tx (the
    // subscribe() call precedes the snapshot send in handle_workflow_socket),
    // so flooding after this point is guaranteed to overflow this connection's
    // receiver rather than racing its setup.
    let snapshot = ws_stream.next().await.unwrap().unwrap();
    let snapshot: serde_json::Value =
        serde_json::from_str(snapshot.into_text().unwrap().as_str()).unwrap();
    assert_eq!(snapshot["type"], "workflowInstanceUpdated");

    // Flood well past the channel capacity with events that don't match this
    // connection's instance filter, then publish one matching marker event.
    flood_noise(&events_tx, BROADCAST_CAPACITY + 50);
    let _ = events_tx.send(BroadcastEvent::NodeStateChanged {
        instance_id: instance_id.clone(),
        node_id: "marker-node".to_string(),
    });

    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let mut saw_lagged = false;
        loop {
            let msg = ws_stream.next().await.unwrap().unwrap();
            let WsMessage::Text(text) = msg else {
                continue;
            };
            let value: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
            if !saw_lagged {
                assert_eq!(value["type"], "lagged", "expected lagged frame first");
                let skipped = value["skipped"].as_u64().expect("skipped is a number");
                assert!(skipped >= 1, "expected skipped >= 1, got {skipped}");
                saw_lagged = true;
                continue;
            }
            // First frame after `lagged` must be the live marker — the stream
            // kept going instead of dying.
            assert_eq!(value["type"], "nodeStateChanged");
            assert_eq!(value["instance_id"], instance_id);
            assert_eq!(value["node_id"], "marker-node");
            break;
        }
    })
    .await;

    assert!(
        result.is_ok(),
        "timed out waiting for lagged frame + resumed live event on workflow WS"
    );
}

/// Test 2: `/stream/logs/{id}/{node_id}/ws` — same overflow/resume contract.
#[tokio::test]
async fn test_logs_ws_delivers_lagged_frame_then_resumes() {
    let backend = make_backend().await;
    let instance_id = Uuid::new_v4().to_string();
    let node_id = "lag-task";
    insert_instance(&backend, &instance_id).await;

    let state = make_state(Arc::clone(&backend));
    let events_tx = state.events_tx.clone();
    let app = newton_core::api::api_v1_router(state);
    let port = spawn_router(app).await;

    let ws_url = format!("ws://127.0.0.1:{port}/stream/logs/{instance_id}/{node_id}/ws");
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect");

    // First frame is the synthetic "Connected to ..." logMessage, sent after
    // handle_logs_socket's initial (unconditional, pre-await) subscribe().
    let connect_frame = ws_stream.next().await.unwrap().unwrap();
    let connect_frame: serde_json::Value =
        serde_json::from_str(connect_frame.into_text().unwrap().as_str()).unwrap();
    assert_eq!(connect_frame["type"], "logMessage");

    // Flood with LogMessage events for a different instance/node (filtered
    // out, never forwarded), then one matching marker line.
    for _ in 0..(BROADCAST_CAPACITY + 50) {
        let _ = events_tx.send(BroadcastEvent::LogMessage {
            instance_id: "flood-noise-instance".to_string(),
            node_id: "flood-noise-node".to_string(),
            message: "noise".to_string(),
            seq: 0,
        });
    }
    let _ = events_tx.send(BroadcastEvent::LogMessage {
        instance_id: instance_id.clone(),
        node_id: node_id.to_string(),
        message: "marker-line".to_string(),
        seq: 0,
    });

    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let mut saw_lagged = false;
        loop {
            let msg = ws_stream.next().await.unwrap().unwrap();
            let WsMessage::Text(text) = msg else {
                continue;
            };
            let value: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
            if !saw_lagged {
                assert_eq!(value["type"], "lagged", "expected lagged frame first");
                let skipped = value["skipped"].as_u64().expect("skipped is a number");
                assert!(skipped >= 1, "expected skipped >= 1, got {skipped}");
                saw_lagged = true;
                continue;
            }
            assert_eq!(value["type"], "logMessage");
            assert_eq!(value["message"], "marker-line");
            break;
        }
    })
    .await;

    assert!(
        result.is_ok(),
        "timed out waiting for lagged frame + resumed live event on logs WS"
    );
}

/// Test 3: `/stream/workflow/{id}/sse` — same overflow/resume contract, over
/// SSE `data:` payloads instead of WS text frames.
#[tokio::test]
async fn test_workflow_sse_delivers_lagged_frame_then_resumes() {
    let backend = make_backend().await;
    let instance_id = Uuid::new_v4().to_string();
    insert_instance(&backend, &instance_id).await;

    let state = make_state(Arc::clone(&backend));
    let events_tx = state.events_tx.clone();
    let app = newton_core::api::api_v1_router(state);
    let port = spawn_router(app).await;

    let client = reqwest::Client::new();
    let mut resp = client
        .get(format!(
            "http://127.0.0.1:{port}/stream/workflow/{instance_id}/sse"
        ))
        .send()
        .await
        .expect("SSE connect");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let mut buf = String::new();

    // Read the connect-time snapshot event. `events_tx.subscribe()` in
    // workflow_sse() runs synchronously before the handler returns the
    // streaming Response, so by the time we have *any* response (let alone a
    // body chunk) the subscription already exists.
    let snapshot = next_sse_json(&mut resp, &mut buf)
        .await
        .expect("snapshot event");
    assert_eq!(snapshot["type"], "workflowInstanceUpdated");

    flood_noise(&events_tx, BROADCAST_CAPACITY + 50);
    let _ = events_tx.send(BroadcastEvent::NodeStateChanged {
        instance_id: instance_id.clone(),
        node_id: "marker-node".to_string(),
    });

    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let lagged = next_sse_json(&mut resp, &mut buf)
            .await
            .expect("lagged event");
        assert_eq!(lagged["type"], "lagged");
        let skipped = lagged["skipped"].as_u64().expect("skipped is a number");
        assert!(skipped >= 1, "expected skipped >= 1, got {skipped}");

        let marker = next_sse_json(&mut resp, &mut buf)
            .await
            .expect("marker event after lagged");
        assert_eq!(marker["type"], "nodeStateChanged");
        assert_eq!(marker["instance_id"], instance_id);
        assert_eq!(marker["node_id"], "marker-node");
    })
    .await;

    assert!(
        result.is_ok(),
        "timed out waiting for lagged frame + resumed live event on workflow SSE"
    );
}

/// Spec 074 PR-5's companion to the lagged-frame tests above: the *other*
/// non-happy-path branch of the same `match rx.recv().await` — `Err(RecvError::Closed)`,
/// reached when every `broadcast::Sender<BroadcastEvent>` clone (there is
/// exactly one, embedded once in the single `AppState` the router is built
/// from — `AppState::new` never clones it, and `Arc<AppState>` clones taken
/// by axum's `State` extractor are pointer refcounts, not `Sender` clones)
/// has been dropped. Aborting the `axum::serve` accept-loop task, letting
/// the per-connection HTTP task exit after handing the raw IO off to the
/// upgraded WS task, leaves the *WS handler task itself* as the only
/// remaining owner of the shared `AppState`/`Sender` — this test's existence
/// (rather than a hang) is itself evidence that this is enough to close the
/// channel out from under it.
#[tokio::test]
async fn test_workflow_ws_ends_cleanly_when_broadcast_channel_closes() {
    let backend = make_backend().await;
    let instance_id = Uuid::new_v4().to_string();
    insert_instance(&backend, &instance_id).await;

    let state = make_state(Arc::clone(&backend));
    let app = newton_core::api::api_v1_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;

    let ws_url = format!("ws://127.0.0.1:{port}/stream/workflow/{instance_id}/ws");
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect");

    let snapshot = ws_stream.next().await.unwrap().unwrap();
    let snapshot: serde_json::Value =
        serde_json::from_str(snapshot.into_text().unwrap().as_str()).unwrap();
    assert_eq!(snapshot["type"], "workflowInstanceUpdated");

    // Nothing else in this test holds a Sender or AppState clone (unlike the
    // lagged-frame tests above, which deliberately keep `events_tx` alive to
    // flood it). Killing the accept loop is the last push needed to drop the
    // router's own state reference.
    server.abort();

    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ws_stream.next().await {
                None => return,
                Some(Ok(WsMessage::Close(_))) => return,
                Some(Ok(_)) => continue,
                Some(Err(_)) => return,
            }
        }
    })
    .await;

    assert!(
        result.is_ok(),
        "timed out waiting for the WS stream to end after the broadcast channel closed"
    );
}

/// Best-effort coverage of the *send-failure* sibling of the Closed branch
/// (`if socket.send(...).await.is_err() { break; }` inside the `Ok(event)`
/// arm): abruptly drop the client's TCP connection (no WS close handshake)
/// so the next server-side write to it fails, then publish a live event.
/// This is inherently racy (there is no direct hook to observe which arm the
/// server took), so this test only asserts the server keeps functioning
/// normally for a fresh connection afterward — it does not itself assert the
/// send-failure branch was hit.
#[tokio::test]
async fn test_workflow_ws_server_survives_client_disconnect_mid_broadcast() {
    let backend = make_backend().await;
    let instance_id = Uuid::new_v4().to_string();
    insert_instance(&backend, &instance_id).await;

    let state = make_state(Arc::clone(&backend));
    let events_tx = state.events_tx.clone();
    let app = newton_core::api::api_v1_router(state);
    let port = spawn_router(app).await;

    let ws_url = format!("ws://127.0.0.1:{port}/stream/workflow/{instance_id}/ws");
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect");
    let snapshot = ws_stream.next().await.unwrap().unwrap();
    let snapshot: serde_json::Value =
        serde_json::from_str(snapshot.into_text().unwrap().as_str()).unwrap();
    assert_eq!(snapshot["type"], "workflowInstanceUpdated");

    // Abrupt drop: no WS close handshake, just the underlying TCP stream
    // going away, so a subsequent server-side write observes a broken pipe.
    drop(ws_stream);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = events_tx.send(BroadcastEvent::NodeStateChanged {
        instance_id: instance_id.clone(),
        node_id: "marker-node".to_string(),
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    // The server must still be healthy: a fresh connection works normally.
    let (mut ws_stream2, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("second WebSocket connect should still succeed");
    let snapshot2 = tokio::time::timeout(Duration::from_secs(5), ws_stream2.next())
        .await
        .expect("no timeout")
        .unwrap()
        .unwrap();
    let snapshot2: serde_json::Value =
        serde_json::from_str(snapshot2.into_text().unwrap().as_str()).unwrap();
    assert_eq!(snapshot2["type"], "workflowInstanceUpdated");
}

/// Same contract as above for `/stream/logs/{id}/{node_id}/ws`
/// (`handle_logs_socket`'s identical `Err(RecvError::Closed) => break` arm).
#[tokio::test]
async fn test_logs_ws_ends_cleanly_when_broadcast_channel_closes() {
    let backend = make_backend().await;
    let instance_id = Uuid::new_v4().to_string();
    let node_id = "closed-branch-task";
    insert_instance(&backend, &instance_id).await;

    let state = make_state(Arc::clone(&backend));
    let app = newton_core::api::api_v1_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    tokio::time::sleep(Duration::from_millis(20)).await;

    let ws_url = format!("ws://127.0.0.1:{port}/stream/logs/{instance_id}/{node_id}/ws");
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect");

    let connect_frame = ws_stream.next().await.unwrap().unwrap();
    let connect_frame: serde_json::Value =
        serde_json::from_str(connect_frame.into_text().unwrap().as_str()).unwrap();
    assert_eq!(connect_frame["type"], "logMessage");

    server.abort();

    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ws_stream.next().await {
                None => return,
                Some(Ok(WsMessage::Close(_))) => return,
                Some(Ok(_)) => continue,
                Some(Err(_)) => return,
            }
        }
    })
    .await;

    assert!(
        result.is_ok(),
        "timed out waiting for the WS stream to end after the broadcast channel closed"
    );
}

/// Reads from an in-flight SSE response until a complete `data: <json>\n\n`
/// event block is found, decodes its payload, and returns it. Skips non-data
/// blocks (e.g. the ": keepalive" comment frame).
async fn next_sse_json(
    resp: &mut reqwest::Response,
    buf: &mut String,
) -> Option<serde_json::Value> {
    loop {
        if let Some(idx) = buf.find("\n\n") {
            let block: String = buf.drain(..idx + 2).collect();
            for line in block.lines() {
                if let Some(payload) = line.strip_prefix("data: ") {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) {
                        return Some(value);
                    }
                }
            }
            continue;
        }
        match resp.chunk().await.ok()? {
            Some(bytes) => buf.push_str(&String::from_utf8_lossy(&bytes)),
            None => return None,
        }
    }
}
