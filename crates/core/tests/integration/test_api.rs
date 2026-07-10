use axum::{
    body::Body,
    http::{header, method::Method, Request, StatusCode},
};
use newton_core::api::state::AppState;
use newton_core::workflow::file_store::FsWorkflowFileStore;
use newton_types::{
    ApiError, BroadcastEvent, HilAction, HilEvent, HilEventType, HilStatus, NodeState, NodeStatus,
    OperatorDescriptor, WorkflowDefinition, WorkflowInstance, WorkflowStatus,
};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

// ── Test helpers ──────────────────────────────────────────────────────────────

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

async fn create_test_state_with_files(workflows_dir: std::path::PathBuf) -> AppState {
    let state = create_test_state().await;
    let store = FsWorkflowFileStore::new(workflows_dir);
    state.with_workflow_files(std::sync::Arc::new(store))
}

/// Insert a WorkflowInstance (and its nodes) into BackendStore.
async fn insert_test_instance(state: &AppState, instance: &WorkflowInstance) {
    state
        .backend
        .upsert_workflow_instance(instance)
        .await
        .expect("upsert_workflow_instance");
    for node in &instance.nodes {
        state
            .backend
            .upsert_node_state(&instance.instance_id, node)
            .await
            .expect("upsert_node_state");
    }
}

/// Insert a HilEvent into BackendStore.
/// Note: requires a corresponding WorkflowInstance to satisfy the FK constraint.
async fn insert_test_hil_event(state: &AppState, event: &HilEvent) {
    // Ensure the parent WorkflowInstance exists (FK requirement)
    let dummy = WorkflowInstance {
        instance_id: event.instance_id.clone(),
        workflow_id: "dummy".to_string(),
        status: WorkflowStatus::Running,
        nodes: vec![],
        started_at: chrono::Utc::now(),
        ended_at: None,
        linked_plan_id: None,
        definition: None,
    };
    // Upsert (no-op if it already exists)
    state
        .backend
        .upsert_workflow_instance(&dummy)
        .await
        .expect("upsert_workflow_instance for hil parent");
    state
        .backend
        .insert_hil_event(event)
        .await
        .expect("insert_hil_event");
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_workflows_empty() {
    let state = create_test_state().await;
    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .uri("/workflows")
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

    insert_test_instance(&state, &instance).await;

    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .uri("/workflows")
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
    let app = newton_core::api::api_v1_router(state);

    let instance_id = Uuid::new_v4();
    let request = Request::builder()
        .uri(format!("/workflows/{}", instance_id))
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
    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .uri("/workflows/invalid-uuid")
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

    insert_test_instance(&state, &instance).await;

    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .uri(format!("/workflows/{}", instance_id))
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

    insert_test_instance(&state, &instance).await;

    let app = newton_core::api::api_v1_router(state);

    let update = json!({"workflow_id": "new-workflow"});

    let request = Request::builder()
        .method(Method::PUT)
        .uri(format!("/workflows/{}", instance_id))
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
async fn test_update_workflow_definition_rejected() {
    // PUT /workflows/{id} must not silently discard `definition`: instance
    // definitions are historical snapshots, and authoring lives in
    // /workflow-files. Sending `definition` is a 422, not a 200-and-drop.
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

    insert_test_instance(&state, &instance).await;

    let backend = state.backend.clone();
    let app = newton_core::api::api_v1_router(state);

    let update = WorkflowDefinition {
        workflow_id: "new-workflow".to_string(),
        definition: json!({"test": "value"}),
    };

    let request = Request::builder()
        .method(Method::PUT)
        .uri(format!("/workflows/{}", instance_id))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&update).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let error: ApiError = serde_json::from_slice(&body).unwrap();

    assert!(
        error.message.contains("/workflow-files"),
        "expected error message to point callers at /workflow-files, got: {}",
        error.message
    );

    // The instance itself must be untouched.
    let stored = backend.get_workflow_instance(&instance_id).await.unwrap();
    assert_eq!(stored.workflow_id, "old-workflow");
}

#[tokio::test]
async fn test_list_operators() {
    let state = create_test_state().await;
    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .uri("/operators")
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
    let app = newton_core::api::api_v1_router(state);

    let instance_id = Uuid::new_v4();
    let request = Request::builder()
        .uri(format!("/hil/workflows/{}", instance_id))
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

    insert_test_hil_event(&state, &event).await;

    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .uri(format!("/hil/workflows/{}", instance_id))
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

    insert_test_hil_event(&state, &event).await;

    let app = newton_core::api::api_v1_router(state);

    let action = HilAction {
        answer: Some("Option A".to_string()),
        response_type: "text".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/hil/workflows/{}/{}/action",
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
    let app = newton_core::api::api_v1_router(state);

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4().to_string();

    let action = HilAction {
        answer: Some("Option A".to_string()),
        response_type: "text".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/hil/workflows/{}/{}/action",
            instance_id, event_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&action).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// Spec 074 B15: an already-resolved HIL event MUST reject a second
/// submission with `409 Conflict` instead of silently re-resolving it. This
/// test previously asserted `200 OK` for this exact scenario — that was
/// documenting a bug (no idempotency guard on the status transition), not
/// intended behavior. The `200` assertion is now the "before" fixture for
/// what the audit (finding B15) called out; the `409` assertion below is
/// the fix under test.
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

    insert_test_hil_event(&state, &event).await;

    let app = newton_core::api::api_v1_router(state);

    let action = HilAction {
        answer: Some("Option A".to_string()),
        response_type: "text".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/hil/workflows/{}/{}/action",
            instance_id, event_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&action).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
}

/// Spec 074 B15: the same non-pending guard applies to `TimedOut` and
/// `Cancelled` events, not just `Resolved` ones.
#[tokio::test]
async fn test_submit_hil_action_timed_out_event_conflicts() {
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
        status: HilStatus::TimedOut,
        timestamp: chrono::Utc::now(),
    };

    insert_test_hil_event(&state, &event).await;

    let app = newton_core::api::api_v1_router(state);

    let action = HilAction {
        answer: Some("Option A".to_string()),
        response_type: "text".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/hil/workflows/{}/{}/action",
            instance_id, event_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&action).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
}

/// Spec 074 B15: a `Question` event can only be resolved with `text` (plus
/// the universal terminal responses); `authorization_approved` is on the
/// flat allow-list but semantically wrong for a question, and must now be
/// rejected with `422` even though it passes the old string-only check.
#[tokio::test]
async fn test_submit_hil_action_authorization_response_rejected_for_question_kind() {
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

    insert_test_hil_event(&state, &event).await;

    let app = newton_core::api::api_v1_router(state);

    let action = HilAction {
        answer: None,
        response_type: "authorization_approved".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/hil/workflows/{}/{}/action",
            instance_id, event_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&action).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// Spec 074 B15: mirror of the previous test in the other direction — a
/// `text` response is on the flat allow-list but semantically wrong for an
/// `Authorization` event.
#[tokio::test]
async fn test_submit_hil_action_text_response_rejected_for_authorization_kind() {
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4().to_string();

    let event = HilEvent {
        event_id: event_id.clone(),
        instance_id: instance_id.clone(),
        node_id: Some("task-1".to_string()),
        channel: "test-channel".to_string(),
        event_type: HilEventType::Authorization,
        question: "Approve this action?".to_string(),
        choices: vec![],
        timeout_seconds: Some(300),
        correlation_id: None,
        status: HilStatus::Pending,
        timestamp: chrono::Utc::now(),
    };

    insert_test_hil_event(&state, &event).await;

    let app = newton_core::api::api_v1_router(state);

    let action = HilAction {
        answer: Some("some free text".to_string()),
        response_type: "text".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/hil/workflows/{}/{}/action",
            instance_id, event_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&action).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// Spec 074 B15: the correct flow for an `Authorization` event
/// (`authorization_approved`) still succeeds under the new kind check.
#[tokio::test]
async fn test_submit_hil_action_authorization_success() {
    let state = create_test_state().await;

    let instance_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4().to_string();

    let event = HilEvent {
        event_id: event_id.clone(),
        instance_id: instance_id.clone(),
        node_id: Some("task-1".to_string()),
        channel: "test-channel".to_string(),
        event_type: HilEventType::Authorization,
        question: "Approve this action?".to_string(),
        choices: vec![],
        timeout_seconds: Some(300),
        correlation_id: None,
        status: HilStatus::Pending,
        timestamp: chrono::Utc::now(),
    };

    insert_test_hil_event(&state, &event).await;

    let app = newton_core::api::api_v1_router(state);

    let action = HilAction {
        answer: None,
        response_type: "authorization_approved".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/hil/workflows/{}/{}/action",
            instance_id, event_id
        ))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&action).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let updated: HilEvent = serde_json::from_slice(&body).unwrap();
    assert_eq!(updated.status, HilStatus::Resolved);
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

    insert_test_hil_event(&state, &event).await;

    let app = newton_core::api::api_v1_router(state);

    let action = HilAction {
        answer: Some("Option A".to_string()),
        response_type: "text".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/hil/workflows/{}/{}/action",
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

    insert_test_hil_event(&state, &event).await;

    let app = newton_core::api::api_v1_router(state);

    let action = HilAction {
        answer: Some("Option A".to_string()),
        response_type: "invalid_type".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/hil/workflows/{}/{}/action",
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

    insert_test_hil_event(&state, &event).await;

    let app = newton_core::api::api_v1_router(state);

    let action = HilAction {
        answer: None,
        response_type: "text".to_string(),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/hil/workflows/{}/{}/action",
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
    let _ = newton_core::api::api_v1_router(state.clone());

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
    let app = newton_core::api::api_v1_router(state);

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
        .uri("/workflows")
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

    insert_test_instance(&state, &instance).await;

    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .method(Method::POST)
        .uri("/workflows")
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
    let app = newton_core::api::api_v1_router(state);

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
        .uri("/workflows")
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

    insert_test_instance(&state, &instance).await;

    let app = newton_core::api::api_v1_router(state);

    let node_update = json!({
        "status": "succeeded",
        "started_at": chrono::Utc::now(),
        "ended_at": chrono::Utc::now()
    });

    let request = Request::builder()
        .method(Method::PATCH)
        .uri(format!("/workflows/{}/nodes/task-1", instance_id))
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
    let app = newton_core::api::api_v1_router(state);

    let instance_id = Uuid::new_v4().to_string();
    let node_update = json!({
        "status": "running",
        "started_at": chrono::Utc::now(),
        "ended_at": null
    });

    let request = Request::builder()
        .method(Method::PATCH)
        .uri(format!("/workflows/{}/nodes/task-1", instance_id))
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

    insert_test_instance(
        &state,
        &WorkflowInstance {
            instance_id: id1.clone(),
            workflow_id: "wf-1".to_string(),
            status: WorkflowStatus::Running,
            nodes: vec![],
            started_at: chrono::Utc::now(),
            ended_at: None,
            definition: None,
            linked_plan_id: None,
        },
    )
    .await;
    insert_test_instance(
        &state,
        &WorkflowInstance {
            instance_id: id2.clone(),
            workflow_id: "wf-2".to_string(),
            status: WorkflowStatus::Succeeded,
            nodes: vec![],
            started_at: chrono::Utc::now(),
            ended_at: Some(chrono::Utc::now()),
            definition: None,
            linked_plan_id: None,
        },
    )
    .await;

    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .uri("/workflows?status=running")
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
        insert_test_instance(
            &state,
            &WorkflowInstance {
                instance_id: id,
                workflow_id: format!("wf-{}", i),
                status: WorkflowStatus::Succeeded,
                nodes: vec![],
                started_at: chrono::Utc::now(),
                ended_at: None,
                definition: None,
                linked_plan_id: None,
            },
        )
        .await;
    }

    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .uri("/workflows?limit=2&offset=1")
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

    insert_test_instance(&state, &instance).await;

    let app = newton_core::api::api_v1_router(state);

    let update = json!({
        "status": "succeeded",
        "ended_at": chrono::Utc::now()
    });

    let request = Request::builder()
        .method(Method::PUT)
        .uri(format!("/workflows/{}", instance_id))
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

    insert_test_instance(&state, &instance).await;

    let app = newton_core::api::api_v1_router(state);

    let node_update = json!({
        "status": "succeeded",
        "started_at": chrono::Utc::now(),
        "ended_at": chrono::Utc::now(),
        "operator_type": "CommandOperator"
    });

    let request = Request::builder()
        .method(Method::PATCH)
        .uri(format!("/workflows/{}/nodes/new-task", instance_id))
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

    insert_test_instance(&state, &instance).await;

    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .uri(format!("/workflows/{}", instance_id))
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

    insert_test_instance(&state, &instance).await;

    let mut rx = state.events_tx.subscribe();

    let app = newton_core::api::api_v1_router(state);

    let node_update = json!({
        "status": "running",
        "started_at": chrono::Utc::now(),
    });

    let request = Request::builder()
        .method(Method::PATCH)
        .uri(format!(
            "/workflows/{}/nodes/new-broadcast-task",
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

// ── Workflow Files API Tests ───────────────────────────────────────────────────

const VALID_WORKFLOW_YAML: &str = r#"version: "2.0"
mode: workflow_graph
workflow:
  settings:
    max_workflow_iterations: 10
  tasks:
    - id: step1
      operator: command
      params:
        command: echo hello
"#;

// A second, distinct-content valid workflow document — used to exercise
// overwriting an existing workflow file (B16/B17 tests) without colliding
// with VALID_WORKFLOW_YAML's content_hash.
const UPDATED_WORKFLOW_YAML: &str = r#"version: "2.0"
mode: workflow_graph
workflow:
  settings:
    max_workflow_iterations: 10
  tasks:
    - id: step1
      operator: command
      params:
        command: echo updated
"#;

// Parseable as WorkflowDocument but will fail semantic validation / lint
const INVALID_SEMANTIC_WORKFLOW_YAML: &str = r#"version: "2.0"
mode: workflow_graph
workflow:
  settings:
    entry_task: nonexistent-task
    max_workflow_iterations: 10
  tasks:
    - id: step1
      operator: completely-unknown-operator-xyz
      params: {}
"#;

#[tokio::test]
async fn test_workflow_files_503_when_not_configured() {
    let state = create_test_state().await;
    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .uri("/workflow-files")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_workflow_files_list_empty() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .uri("/workflow-files")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let items: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(items.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_workflow_files_put_and_get() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    let body = serde_json::json!({
        "content": VALID_WORKFLOW_YAML,
        "expected_hash": null
    });

    let request = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/my-flow")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let resp_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let detail: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
    assert_eq!(detail["name"], "my-flow");
    assert!(detail["content_hash"].is_string());

    // GET it back
    let get_request = Request::builder()
        .uri("/workflow-files/my-flow")
        .body(Body::empty())
        .unwrap();
    let get_response = app.oneshot(get_request).await.unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);

    let get_body = axum::body::to_bytes(get_response.into_body(), usize::MAX)
        .await
        .unwrap();
    let get_detail: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
    assert_eq!(get_detail["name"], "my-flow");
    assert_eq!(get_detail["content"], VALID_WORKFLOW_YAML);
}

#[tokio::test]
async fn test_workflow_files_get_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .uri("/workflow-files/nonexistent")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_workflow_files_put_invalid_yaml() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    let body = serde_json::json!({
        "content": "this: is: not: valid: yaml: {{{",
        "expected_hash": null
    });

    let request = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/bad-flow")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_workflow_files_delete() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    // Create
    let body = serde_json::json!({ "content": VALID_WORKFLOW_YAML, "expected_hash": null });
    let put_request = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/to-delete")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let put_response = app.clone().oneshot(put_request).await.unwrap();
    assert_eq!(put_response.status(), StatusCode::CREATED);

    // Delete
    let del_request = Request::builder()
        .method(Method::DELETE)
        .uri("/workflow-files/to-delete")
        .body(Body::empty())
        .unwrap();
    let del_response = app.clone().oneshot(del_request).await.unwrap();
    assert_eq!(del_response.status(), StatusCode::NO_CONTENT);

    // Confirm gone
    let get_request = Request::builder()
        .uri("/workflow-files/to-delete")
        .body(Body::empty())
        .unwrap();
    let get_response = app.oneshot(get_request).await.unwrap();
    assert_eq!(get_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_workflow_files_delete_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    let request = Request::builder()
        .method(Method::DELETE)
        .uri("/workflow-files/nonexistent")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_workflow_files_validate_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    let body = serde_json::json!({ "content": VALID_WORKFLOW_YAML, "expected_hash": null });

    let request = Request::builder()
        .method(Method::POST)
        .uri("/workflow-files/validate")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let resp_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let diag: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
    assert_eq!(diag["parse_ok"], true);
}

#[tokio::test]
async fn test_workflow_files_list_shows_created_files() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    // Create two files
    for name in &["alpha", "beta"] {
        let body = serde_json::json!({ "content": VALID_WORKFLOW_YAML, "expected_hash": null });
        let req = Request::builder()
            .method(Method::PUT)
            .uri(format!("/workflow-files/{name}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // List
    let list_req = Request::builder()
        .uri("/workflow-files")
        .body(Body::empty())
        .unwrap();
    let list_resp = app.oneshot(list_req).await.unwrap();
    assert_eq!(list_resp.status(), StatusCode::OK);

    let list_body = axum::body::to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let items: serde_json::Value = serde_json::from_slice(&list_body).unwrap();
    assert_eq!(items.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_workflow_files_slug_traversal_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    // GET with traversal slug
    let get_req = Request::builder()
        .uri("/workflow-files/..%2F..%2Fsecret")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // PUT with traversal slug
    let body = serde_json::json!({ "content": VALID_WORKFLOW_YAML, "expected_hash": null });
    let put_req = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/..%2Fetc%2Fpasswd")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let put_resp = app.oneshot(put_req).await.unwrap();
    assert_eq!(put_resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_workflow_files_if_match_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    // Create file first
    let body = serde_json::json!({ "content": VALID_WORKFLOW_YAML, "expected_hash": null });
    let create_req = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/conflict-test")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let create_resp = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);

    // Try to overwrite with wrong If-Match hash
    let body2 = serde_json::json!({
        "content": VALID_WORKFLOW_YAML,
        "expected_hash": "0000000000000000000000000000000000000000000000000000000000000000"
    });
    let conflict_req = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/conflict-test")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body2).unwrap()))
        .unwrap();
    let conflict_resp = app.oneshot(conflict_req).await.unwrap();
    assert_eq!(conflict_resp.status(), StatusCode::CONFLICT);
}

/// Fix 5: a conditional PUT (`If-Match`/`expected_hash` set) against a file
/// that no longer exists must fail CAS with 409, not silently fall through
/// to an unconditional create. Sequence: PUT (create) -> GET (capture hash)
/// -> DELETE -> PUT again with the pre-delete hash as `expected_hash`. The
/// caller's precondition names a file state that's gone; there is no file
/// state left for it to match, so the write must be rejected exactly like a
/// hash mismatch would be, and the file must NOT be resurrected.
#[tokio::test]
async fn test_workflow_files_if_match_put_after_delete_conflicts_not_recreated() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    // Create
    let body = serde_json::json!({ "content": VALID_WORKFLOW_YAML, "expected_hash": null });
    let create_req = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/deleted-then-put")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let create_resp = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);

    // GET to capture the current hash
    let get_req = Request::builder()
        .uri("/workflow-files/deleted-then-put")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = axum::body::to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let detail: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
    let old_hash = detail["content_hash"].as_str().unwrap().to_string();

    // Delete
    let del_req = Request::builder()
        .method(Method::DELETE)
        .uri("/workflow-files/deleted-then-put")
        .body(Body::empty())
        .unwrap();
    let del_resp = app.clone().oneshot(del_req).await.unwrap();
    assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);

    // Conditional PUT with the pre-delete hash: must 409, not 201.
    let body2 = serde_json::json!({
        "content": VALID_WORKFLOW_YAML,
        "expected_hash": old_hash,
    });
    let put_req = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/deleted-then-put")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body2).unwrap()))
        .unwrap();
    let put_resp = app.clone().oneshot(put_req).await.unwrap();
    assert_eq!(
        put_resp.status(),
        StatusCode::CONFLICT,
        "conditional PUT against a deleted file must 409, not silently recreate it"
    );

    // The file must still not exist.
    let get_req2 = Request::builder()
        .uri("/workflow-files/deleted-then-put")
        .body(Body::empty())
        .unwrap();
    let get_resp2 = app.oneshot(get_req2).await.unwrap();
    assert_eq!(
        get_resp2.status(),
        StatusCode::NOT_FOUND,
        "file must not have been recreated by the rejected conditional PUT"
    );
}

#[tokio::test]
async fn test_workflow_files_validate_invalid_semantic() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    let body =
        serde_json::json!({ "content": INVALID_SEMANTIC_WORKFLOW_YAML, "expected_hash": null });

    let request = Request::builder()
        .method(Method::POST)
        .uri("/workflow-files/validate")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let resp_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let diag: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
    assert_eq!(diag["parse_ok"], true);
    // Semantically invalid: either validate_ok is false or lint findings are present
    let validate_ok = diag["validate_ok"].as_bool().unwrap_or(true);
    let lint_empty = diag["lint"]
        .as_array()
        .map(|a| a.is_empty())
        .unwrap_or(true);
    assert!(
        !validate_ok || !lint_empty,
        "expected validation failure or lint findings"
    );
}

#[tokio::test]
async fn test_workflow_files_lenient_save_invalid_semantic() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    let body =
        serde_json::json!({ "content": INVALID_SEMANTIC_WORKFLOW_YAML, "expected_hash": null });

    let put_req = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/lenient-test")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let put_resp = app.oneshot(put_req).await.unwrap();
    // Must be 201 or 200 (not 422) — file IS written even though semantically invalid
    let status = put_resp.status();
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "expected 201 or 200, got {status}"
    );

    let resp_body = axum::body::to_bytes(put_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let detail: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();

    // File exists on disk
    let file_path = dir.path().join("lenient-test.yaml");
    assert!(file_path.exists(), "file should have been written to disk");

    // Diagnostics should reflect semantic invalidity
    let parse_ok = detail["diagnostics"]["parse_ok"].as_bool().unwrap_or(false);
    assert!(parse_ok, "parse_ok should be true");
    let validate_ok = detail["diagnostics"]["validate_ok"]
        .as_bool()
        .unwrap_or(true);
    let lint_empty = detail["diagnostics"]["lint"]
        .as_array()
        .map(|a| a.is_empty())
        .unwrap_or(true);
    assert!(
        !validate_ok || !lint_empty,
        "expected diagnostics to reflect invalid workflow"
    );
}

/// Spec 074 B17 / settled decision 8: overwriting an *existing* workflow
/// file without an `If-Match` header or `expected_hash` body field must be
/// rejected with `428 Precondition Required`, and the file on disk must be
/// left unchanged (the second PUT's content never gets written).
#[tokio::test]
async fn test_workflow_files_put_existing_without_precondition_428() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    // Create the file first (unconditional create is allowed).
    let create_body = serde_json::json!({ "content": VALID_WORKFLOW_YAML, "expected_hash": null });
    let create_req = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/needs-precondition")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
        .unwrap();
    let create_resp = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);

    // Overwrite attempt with neither If-Match nor expected_hash.
    let overwrite_body = serde_json::json!({
        "content": UPDATED_WORKFLOW_YAML,
        "expected_hash": null
    });
    let overwrite_req = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/needs-precondition")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&overwrite_body).unwrap()))
        .unwrap();
    let overwrite_resp = app.clone().oneshot(overwrite_req).await.unwrap();
    assert_eq!(overwrite_resp.status(), StatusCode::PRECONDITION_REQUIRED);

    let resp_body = axum::body::to_bytes(overwrite_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let err: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
    let message = err["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("If-Match") && message.contains("expected_hash"),
        "428 body should name both mechanisms, got: {message}"
    );

    // File must be unchanged (still the original content).
    let get_req = Request::builder()
        .uri("/workflow-files/needs-precondition")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.oneshot(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = axum::body::to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let detail: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
    assert_eq!(detail["content"], VALID_WORKFLOW_YAML);
}

/// Spec 074 B16 + B17: a PUT on an existing file with the correct
/// `If-Match` succeeds (200) and the response's `content_hash`/`modified_at`
/// match a subsequent GET byte-for-byte — proving the write response
/// echoes what the store actually persisted rather than recomputing a hash
/// from the request bytes or stamping `Utc::now()`.
#[tokio::test]
async fn test_workflow_files_put_existing_with_correct_if_match_matches_subsequent_get() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    // Create.
    let create_body = serde_json::json!({ "content": VALID_WORKFLOW_YAML, "expected_hash": null });
    let create_req = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/round-trip")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
        .unwrap();
    let create_resp = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);
    let create_resp_body = axum::body::to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let created: serde_json::Value = serde_json::from_slice(&create_resp_body).unwrap();
    let current_hash = created["content_hash"].as_str().unwrap().to_string();

    // Overwrite with the correct If-Match header carrying the current hash.
    let new_content = UPDATED_WORKFLOW_YAML;
    let update_body = serde_json::json!({ "content": new_content, "expected_hash": null });
    let update_req = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/round-trip")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::IF_MATCH, format!("\"{current_hash}\""))
        .body(Body::from(serde_json::to_vec(&update_body).unwrap()))
        .unwrap();
    let update_resp = app.clone().oneshot(update_req).await.unwrap();
    assert_eq!(update_resp.status(), StatusCode::OK);
    let update_resp_body = axum::body::to_bytes(update_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let put_detail: serde_json::Value = serde_json::from_slice(&update_resp_body).unwrap();

    // Subsequent GET must report the identical content_hash and modified_at
    // that the PUT response claimed.
    let get_req = Request::builder()
        .uri("/workflow-files/round-trip")
        .body(Body::empty())
        .unwrap();
    let get_resp = app.oneshot(get_req).await.unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = axum::body::to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let get_detail: serde_json::Value = serde_json::from_slice(&get_body).unwrap();

    assert_eq!(put_detail["content_hash"], get_detail["content_hash"]);
    assert_eq!(put_detail["modified_at"], get_detail["modified_at"]);
    assert_ne!(
        put_detail["content_hash"], current_hash,
        "content_hash must change since the content changed"
    );
    assert_eq!(get_detail["content"], new_content);
}

/// Spec 074 B17: unconditional PUT of a brand-new file (no If-Match, no
/// expected_hash) must still succeed as a create — only *existing*-file
/// overwrites require a precondition.
#[tokio::test]
async fn test_workflow_files_put_new_file_without_precondition_creates() {
    let dir = tempfile::tempdir().unwrap();
    let state = create_test_state_with_files(dir.path().to_owned()).await;
    let app = newton_core::api::api_v1_router(state);

    let body = serde_json::json!({ "content": VALID_WORKFLOW_YAML, "expected_hash": null });
    let request = Request::builder()
        .method(Method::PUT)
        .uri("/workflow-files/brand-new")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}
