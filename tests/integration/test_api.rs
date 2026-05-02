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
    let state = create_test_state().await;
    let app = newton::api::create_router(state, None);

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

    assert_eq!(json["status"], "ok");
    assert!(json["version"].is_string());
}

#[tokio::test]
async fn test_list_workflows_empty() {
    let state = create_test_state().await;
    let app = newton::api::create_router(state, None);

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
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let instance = WorkflowInstance {
        instance_id: instance_id.clone(),
        workflow_id: "test-workflow".to_string(),
        status: WorkflowStatus::Running,
        nodes: vec![],
        started_at: chrono::Utc::now(),
        ended_at: None,
        definition: None,
        linked_plan_id: None,
    };

    state.instances.insert(instance_id.clone(), instance);

    let app = newton::api::create_router(state, None);

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
    let state = create_test_state().await;
    let app = newton::api::create_router(state, None);

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

    assert_eq!(error.code, "ERR_NOT_FOUND");
    assert_eq!(error.category, "resource");
    assert_eq!(error.message, "Workflow instance not found");
}

#[tokio::test]
async fn test_get_workflow_invalid_id() {
    let state = create_test_state().await;
    let app = newton::api::create_router(state, None);

    let request = Request::builder()
        .uri("/api/workflows/invalid-uuid")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let error: ApiError = serde_json::from_slice(&body).unwrap();

    assert_eq!(error.code, "ERR_VALIDATION");
    assert_eq!(error.category, "validation");
}

#[tokio::test]
async fn test_get_workflow_success() {
    let state = create_test_state().await;

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
            operator_type: None,
        }],
        started_at: chrono::Utc::now(),
        ended_at: None,
        definition: None,
        linked_plan_id: None,
    };

    state.instances.insert(instance_id.clone(), instance);

    let app = newton::api::create_router(state, None);

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
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let instance = WorkflowInstance {
        instance_id: instance_id.clone(),
        workflow_id: "old-workflow".to_string(),
        status: WorkflowStatus::Running,
        nodes: vec![],
        started_at: chrono::Utc::now(),
        ended_at: None,
        definition: None,
        linked_plan_id: None,
    };

    state.instances.insert(instance_id.clone(), instance);

    let app = newton::api::create_router(state, None);

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
    let state = create_test_state().await;
    let app = newton::api::create_router(state, None);

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
    let state = create_test_state().await;
    let app = newton::api::create_router(state, None);

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
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4().to_string();

    let event = HilEvent {
        event_id: event_id.clone(),
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

    state.hil_events.insert(event_id.clone(), event);

    let app = newton::api::create_router(state, None);

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
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4().to_string();

    let event = HilEvent {
        event_id: event_id.clone(),
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

    state.hil_events.insert(event_id.clone(), event);

    let app = newton::api::create_router(state, None);

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
    let state = create_test_state().await;
    let app = newton::api::create_router(state, None);

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4().to_string();

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
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4().to_string();

    let event = HilEvent {
        event_id: event_id.clone(),
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

    state.hil_events.insert(event_id.clone(), event);

    let app = newton::api::create_router(state, None);

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
async fn test_submit_hil_action_accepts_opaque_event_id() {
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let event_id = "opaque-event-id".to_string();

    let event = HilEvent {
        event_id: event_id.clone(),
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

    state.hil_events.insert(event_id.clone(), event);

    let app = newton::api::create_router(state, None);

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
async fn test_submit_hil_action_invalid_response_type() {
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4().to_string();

    let event = HilEvent {
        event_id: event_id.clone(),
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

    state.hil_events.insert(event_id.clone(), event);

    let app = newton::api::create_router(state, None);

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

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_submit_hil_action_missing_answer() {
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4().to_string();

    let event = HilEvent {
        event_id: event_id.clone(),
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

    state.hil_events.insert(event_id.clone(), event);

    let app = newton::api::create_router(state, None);

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

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_event_broadcasting() {
    let state = create_test_state().await;
    let _ = newton::api::create_router(state.clone(), None);

    let instance_id = Uuid::new_v4().to_string();

    let event = BroadcastEvent::WorkflowInstanceUpdated {
        instance_id: instance_id.clone(),
    };

    let _ = state.events_tx.send(event);
}

// ─── Stage 5: New endpoint tests ─────────────────────────────────────────────

#[tokio::test]
async fn test_create_workflow_success() {
    let state = create_test_state().await;
    let app = newton::api::create_router(state, None);

    let instance_id = Uuid::new_v4().to_string();
    let instance = WorkflowInstance {
        instance_id: instance_id.clone(),
        workflow_id: "test-workflow".to_string(),
        status: WorkflowStatus::Running,
        nodes: vec![],
        started_at: chrono::Utc::now(),
        ended_at: None,
        definition: None,
        linked_plan_id: None,
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/workflows")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&instance).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: WorkflowInstance = serde_json::from_slice(&body).unwrap();

    assert_eq!(created.instance_id, instance_id);
    assert_eq!(created.workflow_id, "test-workflow");
}

#[tokio::test]
async fn test_create_workflow_duplicate_returns_409() {
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let instance = WorkflowInstance {
        instance_id: instance_id.clone(),
        workflow_id: "test-workflow".to_string(),
        status: WorkflowStatus::Running,
        nodes: vec![],
        started_at: chrono::Utc::now(),
        ended_at: None,
        definition: None,
        linked_plan_id: None,
    };

    state
        .instances
        .insert(instance_id.clone(), instance.clone());

    let app = newton::api::create_router(state, None);

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/workflows")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&instance).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let error: ApiError = serde_json::from_slice(&body).unwrap();

    assert_eq!(error.code, "ERR_CONFLICT");
}

#[tokio::test]
async fn test_create_workflow_invalid_uuid_returns_422() {
    let state = create_test_state().await;
    let app = newton::api::create_router(state, None);

    let instance = json!({
        "instance_id": "not-a-valid-uuid",
        "workflow_id": "test-workflow",
        "status": "running",
        "nodes": [],
        "started_at": chrono::Utc::now(),
        "ended_at": null
    });

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/workflows")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&instance).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let error: ApiError = serde_json::from_slice(&body).unwrap();

    assert_eq!(error.code, "ERR_VALIDATION");
}

#[tokio::test]
async fn test_update_node_success() {
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let instance = WorkflowInstance {
        instance_id: instance_id.clone(),
        status: WorkflowStatus::Running,
        workflow_id: "test-workflow".to_string(),
        nodes: vec![NodeState {
            node_id: "task-1".to_string(),
            status: NodeStatus::Running,
            started_at: Some(chrono::Utc::now()),
            ended_at: None,
            operator_type: None,
        }],
        started_at: chrono::Utc::now(),
        ended_at: None,
        definition: None,
        linked_plan_id: None,
    };

    state.instances.insert(instance_id.clone(), instance);

    let app = newton::api::create_router(state, None);

    let node_update = json!({
        "status": "succeeded",
        "started_at": chrono::Utc::now(),
        "ended_at": chrono::Utc::now()
    });

    let request = Request::builder()
        .method(Method::PATCH)
        .uri(format!("/api/workflows/{}/nodes/task-1", instance_id))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&node_update).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let updated: WorkflowInstance = serde_json::from_slice(&body).unwrap();

    assert_eq!(updated.nodes.len(), 1);
    assert_eq!(updated.nodes[0].node_id, "task-1");
    assert_eq!(updated.nodes[0].status, NodeStatus::Succeeded);
}

#[tokio::test]
async fn test_update_node_workflow_not_found_returns_404() {
    let state = create_test_state().await;
    let app = newton::api::create_router(state, None);

    let instance_id = Uuid::new_v4().to_string();
    let node_update = json!({
        "status": "running",
        "started_at": chrono::Utc::now(),
        "ended_at": null
    });

    let request = Request::builder()
        .method(Method::PATCH)
        .uri(format!("/api/workflows/{}/nodes/task-1", instance_id))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&node_update).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let error: ApiError = serde_json::from_slice(&body).unwrap();

    assert_eq!(error.code, "ERR_NOT_FOUND");
}

#[tokio::test]
async fn test_list_workflows_filter_by_status() {
    let state = create_test_state().await;

    let id1 = Uuid::new_v4().to_string();
    let id2 = Uuid::new_v4().to_string();

    state.instances.insert(
        id1.clone(),
        WorkflowInstance {
            instance_id: id1.clone(),
            workflow_id: "wf-1".to_string(),
            status: WorkflowStatus::Running,
            nodes: vec![],
            started_at: chrono::Utc::now(),
            ended_at: None,
            definition: None,
            linked_plan_id: None,
        },
    );
    state.instances.insert(
        id2.clone(),
        WorkflowInstance {
            instance_id: id2.clone(),
            workflow_id: "wf-2".to_string(),
            status: WorkflowStatus::Succeeded,
            nodes: vec![],
            started_at: chrono::Utc::now(),
            ended_at: Some(chrono::Utc::now()),
            definition: None,
            linked_plan_id: None,
        },
    );

    let app = newton::api::create_router(state, None);

    let request = Request::builder()
        .uri("/api/workflows?status=running")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let workflows: Vec<WorkflowInstance> = serde_json::from_slice(&body).unwrap();

    assert_eq!(workflows.len(), 1);
    assert_eq!(workflows[0].status, WorkflowStatus::Running);
}

#[tokio::test]
async fn test_list_workflows_pagination() {
    let state = create_test_state().await;

    for i in 0..5 {
        let id = Uuid::new_v4().to_string();
        state.instances.insert(
            id.clone(),
            WorkflowInstance {
                instance_id: id,
                workflow_id: format!("wf-{}", i),
                status: WorkflowStatus::Succeeded,
                nodes: vec![],
                started_at: chrono::Utc::now(),
                ended_at: None,
                definition: None,
                linked_plan_id: None,
            },
        );
    }

    let app = newton::api::create_router(state, None);

    let request = Request::builder()
        .uri("/api/workflows?limit=2&offset=1")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let workflows: Vec<WorkflowInstance> = serde_json::from_slice(&body).unwrap();

    assert_eq!(workflows.len(), 2);
}

#[tokio::test]
async fn test_update_workflow_status() {
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let instance = WorkflowInstance {
        instance_id: instance_id.clone(),
        workflow_id: "test-workflow".to_string(),
        status: WorkflowStatus::Running,
        nodes: vec![],
        started_at: chrono::Utc::now(),
        ended_at: None,
        definition: None,
        linked_plan_id: None,
    };

    state.instances.insert(instance_id.clone(), instance);

    let app = newton::api::create_router(state, None);

    let update = json!({
        "status": "succeeded",
        "ended_at": chrono::Utc::now()
    });

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

    assert_eq!(workflow.status, WorkflowStatus::Succeeded);
    assert!(workflow.ended_at.is_some());
}

async fn create_test_state() -> AppState {
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

    let store = newton_backend::SqliteBackendStore::new_in_memory()
        .await
        .expect("in-memory backend init");
    let backend: std::sync::Arc<dyn newton_backend::BackendStore> = std::sync::Arc::new(store);
    AppState::new(operators, backend)
}

#[tokio::test]
async fn test_update_node_upsert_creates_missing_node() {
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let instance = WorkflowInstance {
        instance_id: instance_id.clone(),
        workflow_id: "test-workflow".to_string(),
        status: WorkflowStatus::Running,
        nodes: vec![NodeState {
            node_id: "existing-task".to_string(),
            status: NodeStatus::Running,
            started_at: Some(chrono::Utc::now()),
            ended_at: None,
            operator_type: None,
        }],
        started_at: chrono::Utc::now(),
        ended_at: None,
        definition: None,
        linked_plan_id: None,
    };

    state.instances.insert(instance_id.clone(), instance);

    let app = newton::api::create_router(state, None);

    let node_update = json!({
        "status": "succeeded",
        "started_at": chrono::Utc::now(),
        "ended_at": chrono::Utc::now(),
        "operator_type": "CommandOperator"
    });

    let request = Request::builder()
        .method(Method::PATCH)
        .uri(format!("/api/workflows/{}/nodes/new-task", instance_id))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&node_update).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let updated: WorkflowInstance = serde_json::from_slice(&body).unwrap();

    assert_eq!(updated.nodes.len(), 2);
    let new_node = updated
        .nodes
        .iter()
        .find(|n| n.node_id == "new-task")
        .unwrap();
    assert_eq!(new_node.status, NodeStatus::Succeeded);
    assert_eq!(new_node.operator_type.as_deref(), Some("CommandOperator"));
}

#[tokio::test]
async fn test_workflow_definition_exposure() {
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let definition = json!({
        "version": "2.0",
        "mode": "workflow_graph",
        "workflow": {
            "settings": {
                "entry_task": "resolve_board_ids",
                "max_time_seconds": 3600,
                "parallel_limit": 4
            },
            "tasks": [
                {
                    "id": "resolve_board_ids",
                    "operator": "GhOperator",
                    "params": {},
                    "transitions": [
                        {"to": "enrich_spec"}
                    ]
                },
                {
                    "id": "enrich_spec",
                    "operator": "AgentOperator",
                    "params": {"model": "claude-3"},
                    "transitions": []
                }
            ]
        }
    });

    let instance = WorkflowInstance {
        instance_id: instance_id.clone(),
        workflow_id: "test-workflow".to_string(),
        status: WorkflowStatus::Running,
        nodes: vec![],
        started_at: chrono::Utc::now(),
        ended_at: None,
        definition: Some(definition.clone()),
        linked_plan_id: None,
    };

    state.instances.insert(instance_id.clone(), instance);

    let app = newton::api::create_router(state, None);

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

    assert!(workflow.definition.is_some());
    let def = workflow.definition.unwrap();
    assert_eq!(def["version"], "2.0");
    assert_eq!(
        def["workflow"]["settings"]["entry_task"],
        "resolve_board_ids"
    );
    let tasks = def["workflow"]["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0]["id"], "resolve_board_ids");
    assert_eq!(tasks[0]["operator"], "GhOperator");
    assert_eq!(tasks[1]["id"], "enrich_spec");
    assert_eq!(tasks[1]["operator"], "AgentOperator");
}

#[tokio::test]
async fn test_node_upsert_broadcasts_event() {
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let instance = WorkflowInstance {
        instance_id: instance_id.clone(),
        workflow_id: "test-workflow".to_string(),
        status: WorkflowStatus::Running,
        nodes: vec![],
        started_at: chrono::Utc::now(),
        ended_at: None,
        definition: None,
        linked_plan_id: None,
    };

    state.instances.insert(instance_id.clone(), instance);

    let mut rx = state.events_tx.subscribe();

    let app = newton::api::create_router(state, None);

    let node_update = json!({
        "status": "running",
        "started_at": chrono::Utc::now(),
    });

    let request = Request::builder()
        .method(Method::PATCH)
        .uri(format!(
            "/api/workflows/{}/nodes/new-broadcast-task",
            instance_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&node_update).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let event = rx.try_recv().unwrap();
    match event {
        BroadcastEvent::NodeStateChanged {
            instance_id: recv_id,
            node_id,
        } => {
            assert_eq!(recv_id, instance_id);
            assert_eq!(node_id, "new-broadcast-task");
        }
        _ => panic!("Expected NodeStateChanged event, got {:?}", event),
    }
}
