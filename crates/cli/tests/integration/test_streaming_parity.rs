//! Integration tests for realtime data parity (WebSocket + SSE) — issue #354.
//!
//! Covers:
//! - G1: heartbeat `/ws` welcome frame and broadcast forwarding
//! - G2: per-instance workflow WS snapshot-on-connect and 404 enforcement
//! - G3: logs WS connect-confirmation line
//!
//! Spawns `newton serve` as a child process and polls `/health` for readiness.

use futures::StreamExt;
use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::tempdir;
use tokio_tungstenite::{connect_async, tungstenite::Message};

fn pick_free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().unwrap().port()
}

fn start_newton_serve(port: u16) -> (std::process::Child, tempfile::TempDir) {
    let dir = tempdir().expect("tempdir");
    let bin = assert_cmd::cargo::cargo_bin("newton");
    let child = Command::new(bin)
        .current_dir(dir.path())
        .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn newton serve");
    (child, dir)
}

async fn wait_for_ready(port: u16) -> bool {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(200))
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while tokio::time::Instant::now() < deadline {
        if let Ok(resp) = client
            .get(format!("http://127.0.0.1:{}/healthz", port))
            .send()
            .await
        {
            if resp.status().is_success() {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    false
}

/// G1: heartbeat `/ws` sends `{"type":"welcome"}` as the first text frame.
#[tokio::test]
async fn ws_heartbeat_welcome() {
    let port = pick_free_port();
    let (mut child, _dir) = start_newton_serve(port);

    let ready = wait_for_ready(port).await;
    if !ready {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not become ready within 20s");
    }

    let url = format!("ws://127.0.0.1:{}/api/v1/ws", port);
    let connect_result =
        tokio::time::timeout(Duration::from_millis(500), connect_async(&url)).await;
    let (mut ws, _) = match connect_result {
        Ok(Ok(pair)) => pair,
        Ok(Err(e)) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("WebSocket connect failed: {e}");
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("WebSocket connect timed out");
        }
    };

    let first = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;

    let _ = child.kill();
    let _ = child.wait();

    match first {
        Ok(Some(Ok(Message::Text(text)))) => {
            assert_eq!(
                text.as_str(),
                r#"{"type":"welcome"}"#,
                "first frame must be welcome"
            );
        }
        other => panic!("expected welcome text frame, got: {other:?}"),
    }
}

/// G1: heartbeat `/ws` forwards broadcast events to connected clients.
#[tokio::test]
async fn ws_heartbeat_forwards_broadcast() {
    let port = pick_free_port();
    let (mut child, _dir) = start_newton_serve(port);

    let ready = wait_for_ready(port).await;
    if !ready {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not become ready within 20s");
    }

    let url = format!("ws://127.0.0.1:{}/api/v1/ws", port);
    let (mut ws, _) = connect_async(&url).await.expect("WS connect /ws");

    // Read and discard the welcome frame.
    let _ = tokio::time::timeout(Duration::from_millis(500), ws.next())
        .await
        .expect("welcome frame timeout")
        .expect("welcome frame present")
        .expect("welcome frame ok");

    // Create a workflow instance (POST /api/v1/workflows).
    let client = reqwest::Client::new();
    let instance_id = uuid::Uuid::new_v4().to_string();
    let body = serde_json::json!({
        "instance_id": instance_id,
        "workflow_id": "test-wf",
        "status": "running",
        "nodes": [],
        "started_at": "2026-01-01T00:00:00Z"
    });
    let create_resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/workflows", port))
        .json(&body)
        .send()
        .await
        .expect("POST /api/v1/workflows");
    assert!(
        create_resp.status().is_success(),
        "create workflow: {}",
        create_resp.status()
    );

    // Trigger NodeStateChanged broadcast via PATCH.
    let patch_body = serde_json::json!({"status": "running"});
    let patch_resp = client
        .patch(format!(
            "http://127.0.0.1:{}/api/v1/workflows/{}/nodes/task-1",
            port, instance_id
        ))
        .json(&patch_body)
        .send()
        .await
        .expect("PATCH node");
    assert!(
        patch_resp.status().is_success(),
        "patch node: {}",
        patch_resp.status()
    );

    // Heartbeat socket must receive the NodeStateChanged event.
    let event_frame = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;

    let _ = child.kill();
    let _ = child.wait();

    match event_frame {
        Ok(Some(Ok(Message::Text(text)))) => {
            assert!(
                text.contains("nodeStateChanged"),
                "expected nodeStateChanged event, got: {text}"
            );
            assert!(
                text.contains(&instance_id),
                "event must contain instance_id, got: {text}"
            );
        }
        other => panic!("expected nodeStateChanged text frame, got: {other:?}"),
    }
}

/// G2: per-instance WS emits `workflowInstanceUpdated` snapshot as first frame.
#[tokio::test]
async fn ws_workflow_snapshot_on_connect() {
    let port = pick_free_port();
    let (mut child, _dir) = start_newton_serve(port);

    let ready = wait_for_ready(port).await;
    if !ready {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not become ready within 20s");
    }

    let client = reqwest::Client::new();
    let instance_id = uuid::Uuid::new_v4().to_string();
    let body = serde_json::json!({
        "instance_id": instance_id,
        "workflow_id": "test-wf",
        "status": "running",
        "nodes": [],
        "started_at": "2026-01-01T00:00:00Z"
    });
    let create_resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/workflows", port))
        .json(&body)
        .send()
        .await
        .expect("POST /api/v1/workflows");
    assert!(create_resp.status().is_success());

    let url = format!(
        "ws://127.0.0.1:{}/api/v1/stream/workflow/{}/ws",
        port, instance_id
    );
    let (mut ws, _) = connect_async(&url)
        .await
        .expect("WS connect workflow stream");

    let first = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;

    let _ = child.kill();
    let _ = child.wait();

    match first {
        Ok(Some(Ok(Message::Text(text)))) => {
            let v: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
            assert_eq!(
                v["type"].as_str(),
                Some("workflowInstanceUpdated"),
                "first frame type: {text}"
            );
            assert_eq!(
                v["instance_id"].as_str(),
                Some(instance_id.as_str()),
                "first frame instance_id: {text}"
            );
        }
        other => panic!("expected workflowInstanceUpdated snapshot, got: {other:?}"),
    }
}

/// G2: workflow WS returns HTTP 404 for unknown instance (no WS upgrade).
///
/// Uses tokio-tungstenite so proper WebSocket handshake headers are sent.
/// The server must return 404 before performing the upgrade; tungstenite
/// surfaces this as `Error::Http(response)`.
#[tokio::test]
async fn ws_workflow_not_found_returns_404() {
    let port = pick_free_port();
    let (mut child, _dir) = start_newton_serve(port);

    let ready = wait_for_ready(port).await;
    if !ready {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not become ready within 20s");
    }

    let unknown_id = uuid::Uuid::new_v4().to_string();
    let url = format!(
        "ws://127.0.0.1:{}/api/v1/stream/workflow/{}/ws",
        port, unknown_id
    );
    let result = connect_async(&url).await;

    let _ = child.kill();
    let _ = child.wait();

    match result {
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            assert_eq!(
                resp.status().as_u16(),
                404,
                "expected HTTP 404, got {}",
                resp.status()
            );
            if let Some(body_bytes) = resp.body() {
                let body_str = String::from_utf8_lossy(body_bytes);
                assert!(
                    body_str.contains("ERR_NOT_FOUND"),
                    "body must contain ERR_NOT_FOUND: {body_str}"
                );
            }
        }
        Ok(_) => panic!("expected 404 rejection, but WS connect succeeded"),
        Err(e) => panic!("expected HTTP 404 error from server, got: {e:?}"),
    }
}

/// G3: logs WS emits `logMessage "Connected to <name>"` as first frame.
#[tokio::test]
async fn ws_logs_connected_line() {
    let port = pick_free_port();
    let (mut child, _dir) = start_newton_serve(port);

    let ready = wait_for_ready(port).await;
    if !ready {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not become ready within 20s");
    }

    let client = reqwest::Client::new();
    let instance_id = uuid::Uuid::new_v4().to_string();
    let body = serde_json::json!({
        "instance_id": instance_id,
        "workflow_id": "test-wf",
        "status": "running",
        "nodes": [],
        "started_at": "2026-01-01T00:00:00Z"
    });
    let create_resp = client
        .post(format!("http://127.0.0.1:{}/api/v1/workflows", port))
        .json(&body)
        .send()
        .await
        .expect("POST /api/v1/workflows");
    assert!(create_resp.status().is_success());

    let node_id = "my-node";
    let url = format!(
        "ws://127.0.0.1:{}/api/v1/stream/logs/{}/{}/ws",
        port, instance_id, node_id
    );
    let (mut ws, _) = connect_async(&url).await.expect("WS connect logs stream");

    let first = tokio::time::timeout(Duration::from_millis(500), ws.next()).await;

    let _ = child.kill();
    let _ = child.wait();

    match first {
        Ok(Some(Ok(Message::Text(text)))) => {
            let v: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
            assert_eq!(
                v["type"].as_str(),
                Some("logMessage"),
                "first frame type: {text}"
            );
            let msg = v["message"].as_str().unwrap_or("");
            assert!(
                msg.starts_with("Connected to "),
                "message must start with 'Connected to ': {msg}"
            );
        }
        other => panic!("expected logMessage connect frame, got: {other:?}"),
    }
}
