use axum::{
    body::Body,
    http::{header, method::Method, Request, StatusCode},
};
use newton::api::state::AppState;
use newton_types::{
    ApiError, BroadcastEvent, HilAction, HilEvent, HilEventType, HilStatus, NodeState, NodeStatus,
    OperatorDescriptor, WorkflowDefinition, WorkflowInstance, WorkflowStatus,
};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn test_health_endpoint() {
    let state = create_test_state();
    let app = newton::api::create_router(state);

    let request = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "healthy");
    assert!(json["version"].is_string());
}

#[tokio::test]
async fn test_list_workflows_empty() {
    let state = create_test_state();
    let app = newton::api::create_router(state);

    let request = Request::builder()
        .uri("/api/workflows")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let workflows: Vec<WorkflowInstance> = serde_json::from_slice(&body).unwrap();

    assert!(workflows.is_empty());
}

#[tokio::test]
async fn test_list_workflows_with_instances() {
    let state = create_test_state();

    let instance_id = Uuid::new_v4().to_string();
    let instance = WorkflowInstance {
        instance_id: instance_id.clone(),
        workflow_id: "test-workflow".to_string(),
        status: WorkflowStatus::Running,
        nodes: vec![],
        started_at: chrono::Utc::now(),
        ended_at: None,
    };

    state.instances.insert(instance_id.clone(), instance);

    let app = newton::api::create_router(state);

    let request = Request::builder()
        .uri("/api/workflows")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let workflows: Vec<WorkflowInstance> = serde_json::from_slice(&body).unwrap();

    assert_eq!(workflows.len(), 1);
    assert_eq!(workflows[0].instance_id, instance_id);
    assert_eq!(workflows[0].workflow_id, "test-workflow");
}

#[tokio::test]
async fn test_get_workflow_not_found() {
    let state = create_test_state();
    let app = newton::api::create_router(state);

    let instance_id = Uuid::new_v4();
    let request = Request::builder()
        .uri(format!("/api/workflows/{}", instance_id))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let error: ApiError = serde_json::from_slice(&body).unwrap();

    assert_eq!(error.code, "API-WORKFLOW-002");
    assert_eq!(error.category, "ValidationError");
    assert_eq!(error.message, "Workflow instance not found");
}

#[tokio::test]
async fn test_get_workflow_invalid_id() {
    let state = create_test_state();
    let app = newton::api::create_router(state);

    let request = Request::builder()
        .uri("/api/workflows/invalid-uuid")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let error: ApiError = serde_json::from_slice(&body).unwrap();

    assert_eq!(error.code, "API-WORKFLOW-001");
    assert_eq!(error.category, "ValidationError");
}

#[tokio::test]
async fn test_get_workflow_success() {
    let state = create_test_state();

    let instance_id = Uuid::new_v4().to_string();
    let instance = WorkflowInstance {
        instance_id: instance_id.clone(),
        workflow_id: "test-workflow".to_string(),
        status: WorkflowStatus::Running,
        nodes: vec![NodeState {
            node_id: "task-1".to_string(),
            status: NodeStatus::Succeeded,
            started_at: Some(chrono::Utc::now()),
            ended_at: Some(chrono::Utc::now()),
        }],
        started_at: chrono::Utc::now(),
        ended_at: None,
    };

    state.instances.insert(instance_id.clone(), instance);

    let app = newton::api::create_router(state);

    let request = Request::builder()
        .uri(format!("/api/workflows/{}", instance_id))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let workflow: WorkflowInstance = serde_json::from_slice(&body).unwrap();

    assert_eq!(workflow.instance_id, instance_id);
    assert_eq!(workflow.workflow_id, "test-workflow");
    assert_eq!(workflow.status, WorkflowStatus::Running);
    assert_eq!(workflow.nodes.len(), 1);
}

#[tokio::test]
async fn test_update_workflow_success() {
    let state = create_test_state();

    let instance_id = Uuid::new_v4().to_string();
    let instance = WorkflowInstance {
        instance_id: instance_id.clone(),
        workflow_id: "old-workflow".to_string(),
        status: WorkflowStatus::Running,
        nodes: vec![],
        started_at: chrono::Utc::now(),
        ended_at: None,
    };

    state.instances.insert(instance_id.clone(), instance);

    let app = newton::api::create_router(state);

    let update = WorkflowDefinition {
        workflow_id: "new-workflow".to_string(),
        definition: json!({"test": "value"}),
    };

    let request = Request::builder()
        .method(Method::PUT)
        .uri(format!("/api/workflows/{}", instance_id))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&update).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let workflow: WorkflowInstance = serde_json::from_slice(&body).unwrap();

    assert_eq!(workflow.workflow_id, "new-workflow");
}

#[tokio::test]
async fn test_list_operators() {
    let state = create_test_state();
    let app = newton::api::create_router(state);

    let request = Request::builder()
        .uri("/api/operators")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let operators: Vec<OperatorDescriptor> = serde_json::from_slice(&body).unwrap();

    assert!(!operators.is_empty());
    assert!(operators.iter().any(|op| op.operator_type == "noop"));
}

#[tokio::test]
async fn test_list_hil_events_empty() {
    let state = create_test_state();
    let app = newton::api::create_router(state);

    let instance_id = Uuid::new_v4();
    let request = Request::builder()
        .uri(format!("/api/hil/workflows/{}", instance_id))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let events: Vec<HilEvent> = serde_json::from_slice(&body).unwrap();

    assert!(events.is_empty());
}

#[tokio::test]
async fn test_list_hil_events_with_events() {
    let state = create_test_state();

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4();

    let event = HilEvent {
        event_id,
        instance_id: instance_id.clone(),
        node_id: Some("task-1".to_string()),
        channel: "test-channel".to_string(),
        event_type: HilEventType::Question,
        question: "What should we do?".to_string(),
        choices: vec!["Option A".to_string(), "Option B".to_string()],
        timeout_seconds: Some(300),
        correlation_id: None,
        status: HilStatus::Pending,
        timestamp: chrono::Utc::now(),
    };

    state.hil_events.insert(event_id, event);

    let app = newton::api::create_router(state);

    let request = Request::builder()
        .uri(format!("/api/hil/workflows/{}", instance_id))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let events: Vec<HilEvent> = serde_json::from_slice(&body).unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_id, event_id);
    assert_eq!(events[0].instance_id, instance_id);
}

#[tokio::test]
async fn test_submit_hil_action_success() {
    let state = create_test_state();

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4();

    let event = HilEvent {
        event_id,
        instance_id: instance_id.clone(),
        node_id: Some("task-1".to_string()),
        channel: "test-channel".to_string(),
        event_type: HilEventType::Question,
        question: "What should we do?".to_string(),
        choices: vec!["Option A".to_string(), "Option B".to_string()],
        timeout_seconds: Some(300),
        correlation_id: None,
        status: HilStatus::Pending,
        timestamp: chrono::Utc::now(),
    };

    state.hil_events.insert(event_id, event);

    let app = newton::api::create_router(state);

    let action = HilAction {
        answer: Some("Option A".to_string()),
        response_type: "text".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/hil/workflows/{}/{}/action",
            instance_id, event_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&action).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_submit_hil_action_not_found() {
    let state = create_test_state();
    let app = newton::api::create_router(state);

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4();

    let action = HilAction {
        answer: Some("Option A".to_string()),
        response_type: "text".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/hil/workflows/{}/{}/action",
            instance_id, event_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&action).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_submit_hil_action_already_resolved() {
    let state = create_test_state();

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4();

    let event = HilEvent {
        event_id,
        instance_id: instance_id.clone(),
        node_id: Some("task-1".to_string()),
        channel: "test-channel".to_string(),
        event_type: HilEventType::Question,
        question: "What should we do?".to_string(),
        choices: vec!["Option A".to_string(), "Option B".to_string()],
        timeout_seconds: Some(300),
        correlation_id: None,
        status: HilStatus::Resolved,
        timestamp: chrono::Utc::now(),
    };

    state.hil_events.insert(event_id, event);

    let app = newton::api::create_router(state);

    let action = HilAction {
        answer: Some("Option A".to_string()),
        response_type: "text".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/hil/workflows/{}/{}/action",
            instance_id, event_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&action).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_submit_hil_action_invalid_response_type() {
    let state = create_test_state();

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4();

    let event = HilEvent {
        event_id,
        instance_id: instance_id.clone(),
        node_id: Some("task-1".to_string()),
        channel: "test-channel".to_string(),
        event_type: HilEventType::Question,
        question: "What should we do?".to_string(),
        choices: vec!["Option A".to_string(), "Option B".to_string()],
        timeout_seconds: Some(300),
        correlation_id: None,
        status: HilStatus::Pending,
        timestamp: chrono::Utc::now(),
    };

    state.hil_events.insert(event_id, event);

    let app = newton::api::create_router(state);

    let action = HilAction {
        answer: Some("Option A".to_string()),
        response_type: "invalid_type".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/hil/workflows/{}/{}/action",
            instance_id, event_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&action).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_submit_hil_action_missing_answer() {
    let state = create_test_state();

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4();

    let event = HilEvent {
        event_id,
        instance_id: instance_id.clone(),
        node_id: Some("task-1".to_string()),
        channel: "test-channel".to_string(),
        event_type: HilEventType::Question,
        question: "What should we do?".to_string(),
        choices: vec!["Option A".to_string(), "Option B".to_string()],
        timeout_seconds: Some(300),
        correlation_id: None,
        status: HilStatus::Pending,
        timestamp: chrono::Utc::now(),
    };

    state.hil_events.insert(event_id, event);

    let app = newton::api::create_router(state);

    let action = HilAction {
        answer: None,
        response_type: "text".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/hil/workflows/{}/{}/action",
            instance_id, event_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&action).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_workflow_stream_invalid_uuid() {
    let state = create_test_state();
    let app = newton::api::create_router(state);

    let request = Request::builder()
        .uri("/api/stream/workflow/invalid-uuid/ws")
        .header(header::UPGRADE, "websocket")
        .header(header::CONNECTION, "upgrade")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_legacy_list_channels() {
    let state = create_test_state();

    let instance_id = Uuid::new_v4().to_string();
    let instance = WorkflowInstance {
        instance_id: instance_id.clone(),
        workflow_id: "test-workflow".to_string(),
        status: WorkflowStatus::Running,
        nodes: vec![],
        started_at: chrono::Utc::now(),
        ended_at: None,
    };

    state.instances.insert(instance_id.clone(), instance);

    let app = newton::api::create_router(state);

    let request = Request::builder()
        .uri("/channels")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["channels"].is_array());
    assert_eq!(json["channels"].as_array().unwrap().len(), 1);
    assert_eq!(json["channels"][0], "test-workflow");
}

#[tokio::test]
async fn test_legacy_api_list_channels() {
    let state = create_test_state();

    let instance_id = Uuid::new_v4().to_string();
    state.instances.insert(
        instance_id.clone(),
        WorkflowInstance {
            instance_id: instance_id.clone(),
            workflow_id: "workflow-a".to_string(),
            status: WorkflowStatus::Running,
            nodes: vec![],
            started_at: chrono::Utc::now(),
            ended_at: None,
        },
    );

    let app = newton::api::create_router(state);
    let request = Request::builder()
        .uri("/api/channels")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["channels"].is_array());
    assert_eq!(json["channels"][0]["name"], "workflow-a");
}

#[tokio::test]
async fn test_legacy_api_list_channel_messages() {
    let state = create_test_state();

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4();
    state.hil_events.insert(
        event_id,
        HilEvent {
            event_id,
            instance_id,
            node_id: Some("task-1".to_string()),
            channel: "workflow-a".to_string(),
            event_type: HilEventType::Question,
            question: "Proceed?".to_string(),
            choices: vec!["yes".to_string(), "no".to_string()],
            timeout_seconds: Some(30),
            correlation_id: None,
            status: HilStatus::Pending,
            timestamp: chrono::Utc::now(),
        },
    );

    let app = newton::api::create_router(state);
    let request = Request::builder()
        .uri("/api/channels/workflow-a/messages?limit=10")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 1);
    assert_eq!(json[0]["id"], event_id.to_string());
    assert_eq!(json[0]["content"]["type"], "question");
}

#[tokio::test]
async fn test_legacy_api_submit_message_response() {
    let state = create_test_state();

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4();
    state.hil_events.insert(
        event_id,
        HilEvent {
            event_id,
            instance_id,
            node_id: Some("task-1".to_string()),
            channel: "workflow-a".to_string(),
            event_type: HilEventType::Question,
            question: "Proceed?".to_string(),
            choices: vec!["yes".to_string(), "no".to_string()],
            timeout_seconds: Some(30),
            correlation_id: None,
            status: HilStatus::Pending,
            timestamp: chrono::Utc::now(),
        },
    );

    let app = newton::api::create_router(state);
    let action = HilAction {
        answer: Some("yes".to_string()),
        response_type: "text".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/v1/messages/{}/response", event_id))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&action).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["event_id"], event_id.to_string());
}

#[tokio::test]
async fn test_event_broadcasting() {
    let state = create_test_state();
    let _ = newton::api::create_router(state.clone());

    let instance_id = Uuid::new_v4().to_string();

    let event = BroadcastEvent::WorkflowInstanceUpdated {
        instance_id: instance_id.clone(),
    };

    let _ = state.events_tx.send(event);
}

fn create_test_state() -> AppState {
    let operators = vec![
        OperatorDescriptor {
            operator_type: "noop".to_string(),
            description: "No-operation operator".to_string(),
            params_schema: json!({}),
        },
        OperatorDescriptor {
            operator_type: "command".to_string(),
            description: "Execute shell commands".to_string(),
            params_schema: json!({"type": "object"}),
        },
    ];

    AppState::new(operators)
}
