/// Integration tests verifying persistence across AppState restart and log replay.
use axum::{
    body::Body,
    http::{header, method::Method, Request, StatusCode},
};
use newton_backend::BackendStore;
use newton_core::api::state::AppState;
use newton_types::{
    HilEvent, HilEventType, HilStatus, LogLine, NodeStatus, OperatorDescriptor, WorkflowInstance,
    WorkflowStatus,
};
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;
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

/// Test A: POST a workflow instance, PATCH 2 nodes, drop AppState (simulate restart by
/// creating a new AppState over the same backend), GET instance — both node states intact.
#[tokio::test]
async fn test_restart_persistence() {
    let backend = make_backend().await;
    let instance_id = Uuid::new_v4().to_string();

    // ── Phase 1: create state and populate ───────────────────────────────────
    {
        let state = make_state(Arc::clone(&backend));
        let app = newton_core::api::api_v1_router(state, false);

        // POST workflow
        let instance = WorkflowInstance {
            instance_id: instance_id.clone(),
            workflow_id: "wf-restart".to_string(),
            status: WorkflowStatus::Running,
            nodes: vec![],
            started_at: chrono::Utc::now(),
            ended_at: None,
            definition: None,
            linked_plan_id: None,
        };
        let req = Request::builder()
            .method(Method::POST)
            .uri("/workflows")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&instance).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // PATCH node-a
        let patch_a = json!({"status": "running", "started_at": chrono::Utc::now()});
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(format!("/workflows/{}/nodes/node-a", instance_id))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&patch_a).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // PATCH node-b
        let patch_b = json!({"status": "succeeded", "started_at": chrono::Utc::now(), "ended_at": chrono::Utc::now()});
        let req = Request::builder()
            .method(Method::PATCH)
            .uri(format!("/workflows/{}/nodes/node-b", instance_id))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&patch_b).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ── Phase 2: new AppState over the same backend (simulate restart) ───────
    {
        let state2 = make_state(Arc::clone(&backend));
        let app2 = newton_core::api::api_v1_router(state2, false);

        let req = Request::builder()
            .uri(format!("/workflows/{}", instance_id))
            .body(Body::empty())
            .unwrap();
        let resp = app2.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let fetched: WorkflowInstance = serde_json::from_slice(&body).unwrap();

        assert_eq!(fetched.instance_id, instance_id);
        assert_eq!(fetched.workflow_id, "wf-restart");
        assert_eq!(fetched.nodes.len(), 2);
        assert!(fetched.nodes.iter().any(|n| n.node_id == "node-a"));
        assert!(fetched.nodes.iter().any(|n| n.node_id == "node-b"));
        let node_b = fetched
            .nodes
            .iter()
            .find(|n| n.node_id == "node-b")
            .unwrap();
        assert_eq!(node_b.status, NodeStatus::Succeeded);
    }
}

/// Test B: Append N ≥ 10 log lines, restart AppState, connect to the logs WebSocket,
/// assert all N historical lines are received (in seq order) before any live events.
#[tokio::test]
async fn test_log_replay_after_restart() {
    use futures::StreamExt;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    let backend = make_backend().await;
    let instance_id = Uuid::new_v4().to_string();
    let node_id = "task-log";
    const N: usize = 12;

    // Insert parent instance (FK requirement for NodeState and WorkflowLog)
    backend
        .upsert_workflow_instance(&WorkflowInstance {
            instance_id: instance_id.clone(),
            workflow_id: "wf-log".to_string(),
            status: WorkflowStatus::Running,
            nodes: vec![],
            started_at: chrono::Utc::now(),
            ended_at: None,
            definition: None,
            linked_plan_id: None,
        })
        .await
        .unwrap();

    // Append N log lines before the "restart"
    for i in 0..N {
        let line = LogLine {
            instance_id: instance_id.clone(),
            node_id: node_id.to_string(),
            level: "info".to_string(),
            message: format!("log-line-{i}"),
            timestamp: chrono::Utc::now(),
            // append_log_line assigns the real seq; this placeholder is
            // never read on the write path.
            seq: 0,
        };
        backend
            .append_log_line(&instance_id, node_id, &line)
            .await
            .unwrap();
    }

    // ── Simulate restart: new AppState over the same backend ─────────────────
    let state2 = make_state(Arc::clone(&backend));
    let app2 = newton_core::api::api_v1_router(state2, false);

    // Bind to an ephemeral port and spawn the axum server in the background
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app2.into_make_service())
            .await
            .unwrap();
    });

    // Give the server a moment to start accepting connections
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    // Connect to the logs WebSocket endpoint
    let ws_url = format!("ws://127.0.0.1:{port}/stream/logs/{instance_id}/{node_id}/ws");
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect");

    // Collect log message frames.
    // handle_logs_socket emits:
    //   1. A "Connected to …" LogMessage (the join frame — skip it)
    //   2. N historical LogMessage frames (the replay — assert these)
    //   3. Any live broadcast events (we stop before any arrive)
    let mut historical: Vec<String> = Vec::new();
    let mut skip_connect = true;

    let collect = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(Ok(msg)) = ws_stream.next().await {
            if let WsMessage::Text(text) = msg {
                let event: serde_json::Value = serde_json::from_str(&text).unwrap();
                if event["type"] == "logMessage" {
                    if skip_connect {
                        // First logMessage is the "Connected to …" join frame
                        skip_connect = false;
                        continue;
                    }
                    historical.push(event["message"].as_str().unwrap().to_string());
                    if historical.len() == N {
                        break;
                    }
                }
            }
        }
    })
    .await;

    assert!(
        collect.is_ok(),
        "Timed out waiting for {N} historical log lines from logs WebSocket"
    );

    // All N historical lines must arrive in seq order before any live events
    assert_eq!(historical.len(), N, "Expected {N} historical log lines");
    for (i, msg) in historical.iter().enumerate() {
        assert_eq!(
            msg,
            &format!("log-line-{i}"),
            "Historical log line {i} out of order"
        );
    }

    server_handle.abort();
}

/// Test C: POST a HIL event, submit an action (resolve), restart, GET HIL events for
/// instance — status is "resolved".
#[tokio::test]
async fn test_hil_persistence_after_restart() {
    let backend = make_backend().await;
    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4().to_string();

    // Insert parent workflow instance first (FK requirement)
    backend
        .upsert_workflow_instance(&WorkflowInstance {
            instance_id: instance_id.clone(),
            workflow_id: "wf-hil".to_string(),
            status: WorkflowStatus::Running,
            nodes: vec![],
            started_at: chrono::Utc::now(),
            ended_at: None,
            definition: None,
            linked_plan_id: None,
        })
        .await
        .unwrap();

    // Phase 1: insert HIL event and submit action
    {
        let state = make_state(Arc::clone(&backend));
        let app = newton_core::api::api_v1_router(state, false);

        // Insert HIL event via backend (no HTTP endpoint for creating HIL events externally)
        backend
            .insert_hil_event(&HilEvent {
                event_id: event_id.clone(),
                instance_id: instance_id.clone(),
                node_id: Some("hil-task".to_string()),
                channel: "test".to_string(),
                event_type: HilEventType::Question,
                question: "Proceed?".to_string(),
                choices: vec!["Yes".to_string(), "No".to_string()],
                timeout_seconds: None,
                correlation_id: None,
                status: HilStatus::Pending,
                timestamp: chrono::Utc::now(),
            })
            .await
            .unwrap();

        // Submit action to resolve
        let action = json!({"answer": "Yes", "response_type": "text"});
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/hil/workflows/{}/{}/action",
                instance_id, event_id
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&action).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // Phase 2: restart — new AppState over the same backend
    {
        let state2 = make_state(Arc::clone(&backend));
        let app2 = newton_core::api::api_v1_router(state2, false);

        let req = Request::builder()
            .uri(format!("/hil/workflows/{}", instance_id))
            .body(Body::empty())
            .unwrap();
        let resp = app2.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let events: Vec<HilEvent> = serde_json::from_slice(&body).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, event_id);
        assert_eq!(events[0].status, HilStatus::Resolved);
    }
}

/// Test D: POST 3 workflow instances with different statuses, restart, GET with
/// ?status=running — correct filtering.
#[tokio::test]
async fn test_list_after_restart_with_filter() {
    let backend = make_backend().await;

    let id_running1 = Uuid::new_v4().to_string();
    let id_running2 = Uuid::new_v4().to_string();
    let id_succeeded = Uuid::new_v4().to_string();

    // Phase 1: insert 3 instances
    {
        let state = make_state(Arc::clone(&backend));
        let app = newton_core::api::api_v1_router(state, false);

        for (id, status) in [
            (id_running1.clone(), "running"),
            (id_running2.clone(), "running"),
            (id_succeeded.clone(), "succeeded"),
        ] {
            let body = json!({
                "instance_id": id,
                "workflow_id": "wf-filter",
                "status": status,
                "nodes": [],
                "started_at": chrono::Utc::now(),
                "ended_at": null
            });
            let req = Request::builder()
                .method(Method::POST)
                .uri("/workflows")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::CREATED);
        }
    }

    // Phase 2: restart — new AppState, check filtered list
    {
        let state2 = make_state(Arc::clone(&backend));
        let app2 = newton_core::api::api_v1_router(state2, false);

        let req = Request::builder()
            .uri("/workflows?status=running")
            .body(Body::empty())
            .unwrap();
        let resp = app2.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let workflows: Vec<WorkflowInstance> = serde_json::from_slice(&body).unwrap();

        assert_eq!(workflows.len(), 2);
        for wf in &workflows {
            assert_eq!(wf.status, WorkflowStatus::Running);
        }

        // Also verify the full list has 3
        let req = Request::builder()
            .uri("/workflows")
            .body(Body::empty())
            .unwrap();
        let state3 = make_state(Arc::clone(&backend));
        let app3 = newton_core::api::api_v1_router(state3, false);
        let resp = app3.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let all: Vec<WorkflowInstance> = serde_json::from_slice(&body).unwrap();
        assert_eq!(all.len(), 3);
    }
}
