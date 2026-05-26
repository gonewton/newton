use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use newton_core::api::state::AppState;
use newton_types::OperatorDescriptor;
use serde_json::json;
use tower::ServiceExt;

async fn create_test_state() -> AppState {
    let operators = vec![OperatorDescriptor {
        operator_type: "noop".to_string(),
        description: "No-operation operator".to_string(),
        params_schema: json!({}),
    }];
    let store = newton_backend::SqliteBackendStore::new_in_memory()
        .await
        .expect("in-memory backend init");
    let backend: std::sync::Arc<dyn newton_backend::BackendStore> = std::sync::Arc::new(store);
    AppState::new(operators, backend)
}

// AC3: GET /aitools → 200, lists at least the "newton/ping" tool
#[tokio::test]
async fn test_list_aitools_returns_200() {
    let state = create_test_state().await;
    let app = newton_core::api::api_v1_router(state);
    let req = Request::builder()
        .uri("/aitools")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let tools = body["tools"].as_array().expect("tools must be an array");
    let has_ping = tools
        .iter()
        .any(|t| t["namespace"] == "newton" && t["name"] == "ping");
    assert!(has_ping, "expected newton/ping in tool list, got: {body}");
}

// AC4: GET /aitools/newton/ping/schema → 200, valid JSON schema body
#[tokio::test]
async fn test_ping_schema_returns_200() {
    let state = create_test_state().await;
    let app = newton_core::api::api_v1_router(state);
    let req = Request::builder()
        .uri("/aitools/newton/ping/schema")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["namespace"], "newton");
    assert_eq!(body["name"], "ping");
    assert!(
        body["inputSchema"].is_object(),
        "inputSchema must be object"
    );
    assert!(
        body["outputSchema"].is_object(),
        "outputSchema must be object"
    );
}

// AC5: POST /aitools/newton/ping with valid payload → 200
#[tokio::test]
async fn test_ping_invoke_returns_200() {
    let state = create_test_state().await;
    let app = newton_core::api::api_v1_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/aitools/newton/ping")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["draft"]["pong"], true);
}

// AC7: SSE session endpoint returns content-type: text/event-stream
#[tokio::test]
async fn test_ping_sessions_messages_sse_content_type() {
    let state = create_test_state().await;
    let app = newton_core::api::api_v1_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/aitools/newton/ping/sessions/test-session/messages")
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(Body::from(json!({"content": "hello"}).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "expected text/event-stream, got: {content_type}"
    );
}
