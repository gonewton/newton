pub mod hil;
pub mod operators_api;
pub mod state;
pub mod streaming_api;
pub mod workflows;

use crate::api::state::AppState;
use axum::{
    extract::Path,
    extract::Query,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response, Sse},
    routing::{get, patch, post, put},
    Router,
};
use chrono::{DateTime, Utc};
use newton_types::{
    ApiError, BroadcastEvent, HilAction, HilEvent, HilStatus, NodeStatus, WorkflowInstance,
    WorkflowStatus,
};
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

pub fn create_router(state: AppState, ui_dir: Option<PathBuf>) -> Router {
    let arc_state = Arc::new(state);

    let mut router = Router::new()
        .route("/health", get(health_check))
        .route("/api/workflows", get(list_workflows))
        .route("/api/workflows", post(create_workflow))
        .route("/api/workflows/{id}", get(get_workflow))
        .route("/api/workflows/{id}", put(update_workflow))
        .route("/api/workflows/{id}/nodes/{node_id}", patch(update_node))
        .route("/api/hil/workflows/{id}", get(list_hil_events))
        .route(
            "/api/hil/workflows/{id}/{event_id}/action",
            post(submit_hil_action),
        )
        .route("/api/channels", get(legacy_list_channels_v1))
        .route(
            "/api/channels/{channel}/messages",
            get(legacy_list_channel_messages),
        )
        .route(
            "/api/v1/messages/{event_id}/response",
            post(legacy_submit_message_response),
        )
        .route("/api/operators", get(list_operators))
        .route("/api/stream/workflow/{id}/ws", get(workflow_stream))
        .route("/api/stream/logs/{id}/{node_id}/ws", get(logs_stream))
        .route("/api/stream/workflow/{id}/sse", get(workflow_sse))
        .route("/channels", get(legacy_list_channels))
        .with_state(arc_state)
        .layer(CorsLayer::permissive());

    if let Some(ref dir) = ui_dir {
        if dir.exists() {
            router = router.fallback_service(
                ServeDir::new(dir).not_found_service(ServeFile::new(dir.join("index.html"))),
            );
        }
    }

    router
}

async fn health_check() -> impl IntoResponse {
    Json(json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

#[derive(Debug, Deserialize)]
struct WorkflowQuery {
    status: Option<WorkflowStatus>,
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct NodeUpdate {
    status: NodeStatus,
    started_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
    operator_type: Option<String>,
}

/// Flexible update body: supports both legacy WorkflowDefinition format
/// and new status/ended_at update format.
#[derive(Debug, Deserialize)]
struct WorkflowUpdateBody {
    workflow_id: Option<String>,
    #[allow(dead_code)]
    definition: Option<serde_json::Value>,
    status: Option<WorkflowStatus>,
    ended_at: Option<DateTime<Utc>>,
}

async fn list_workflows(
    Query(query): Query<WorkflowQuery>,
    State(state): State<Arc<AppState>>,
) -> Json<Vec<WorkflowInstance>> {
    let mut instances: Vec<WorkflowInstance> = state
        .instances
        .iter()
        .map(|entry| entry.value().clone())
        .collect();

    if let Some(ref status) = query.status {
        instances.retain(|i| &i.status == status);
    }

    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(usize::MAX);
    instances = instances.into_iter().skip(offset).take(limit).collect();

    Json(instances)
}

async fn get_workflow(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    match Uuid::parse_str(&id) {
        Ok(_) => {}
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "API-WORKFLOW-001".to_string(),
                    category: "ValidationError".to_string(),
                    message: "Invalid workflow instance ID format".to_string(),
                    details: None,
                }),
            )
                .into_response()
        }
    }

    match state.instances.get(&id) {
        Some(instance) => (StatusCode::OK, Json(instance.clone())).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "API-WORKFLOW-002".to_string(),
                category: "ValidationError".to_string(),
                message: "Workflow instance not found".to_string(),
                details: None,
            }),
        )
            .into_response(),
    }
}

async fn create_workflow(
    State(state): State<Arc<AppState>>,
    Json(instance): Json<WorkflowInstance>,
) -> Response {
    if Uuid::parse_str(&instance.instance_id).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "API-WORKFLOW-001".to_string(),
                category: "ValidationError".to_string(),
                message: "Invalid workflow instance ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    if state.instances.contains_key(&instance.instance_id) {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                code: "API-WORKFLOW-003".to_string(),
                category: "ValidationError".to_string(),
                message: "Workflow instance already exists".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    state
        .instances
        .insert(instance.instance_id.clone(), instance.clone());
    (StatusCode::CREATED, Json(instance)).into_response()
}

async fn update_workflow(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<WorkflowUpdateBody>,
) -> Response {
    if Uuid::parse_str(&id).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "API-WORKFLOW-001".to_string(),
                category: "ValidationError".to_string(),
                message: "Invalid workflow instance ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    if let Some(mut instance) = state.instances.get_mut(&id) {
        if let Some(workflow_id) = body.workflow_id {
            instance.workflow_id = workflow_id;
        }
        if let Some(status) = body.status {
            instance.status = status;
        }
        if let Some(ended_at) = body.ended_at {
            instance.ended_at = Some(ended_at);
        }
        (StatusCode::OK, Json(instance.clone())).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "API-WORKFLOW-002".to_string(),
                category: "ValidationError".to_string(),
                message: "Workflow instance not found".to_string(),
                details: None,
            }),
        )
            .into_response()
    }
}

async fn update_node(
    Path((id, node_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(node_update): Json<NodeUpdate>,
) -> Response {
    if Uuid::parse_str(&id).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "API-WORKFLOW-001".to_string(),
                category: "ValidationError".to_string(),
                message: "Invalid workflow instance ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    if node_id.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "API-NODE-001".to_string(),
                category: "ValidationError".to_string(),
                message: "Invalid node ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    match state.instances.get_mut(&id) {
        Some(mut instance) => {
            if let Some(node) = instance.nodes.iter_mut().find(|n| n.node_id == node_id) {
                node.status = node_update.status;
                if node_update.started_at.is_some() {
                    node.started_at = node_update.started_at;
                }
                node.ended_at = node_update.ended_at;
                if node_update.operator_type.is_some() {
                    node.operator_type = node_update.operator_type;
                }
            } else {
                let new_node = newton_types::NodeState {
                    node_id: node_id.clone(),
                    status: node_update.status,
                    started_at: node_update.started_at,
                    ended_at: node_update.ended_at,
                    operator_type: node_update.operator_type,
                };
                instance.nodes.push(new_node);
            }

            let _ = state.events_tx.send(BroadcastEvent::NodeStateChanged {
                instance_id: id.clone(),
                node_id: node_id.clone(),
            });

            (StatusCode::OK, Json(instance.clone())).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "API-WORKFLOW-002".to_string(),
                category: "ValidationError".to_string(),
                message: "Workflow instance not found".to_string(),
                details: None,
            }),
        )
            .into_response(),
    }
}

async fn list_hil_events(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    let events: Vec<HilEvent> = state
        .hil_events
        .iter()
        .filter(|entry| entry.value().instance_id == id)
        .map(|entry| entry.value().clone())
        .collect();
    (StatusCode::OK, Json(events)).into_response()
}

async fn submit_hil_action(
    Path((instance_id, event_id)): Path<(String, Uuid)>,
    State(state): State<Arc<AppState>>,
    Json(action): Json<HilAction>,
) -> Response {
    match state.hil_events.get_mut(&event_id) {
        Some(mut hil_event) => {
            if hil_event.instance_id != instance_id {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ApiError {
                        code: "API-HIL-001".to_string(),
                        category: "ValidationError".to_string(),
                        message: "HIL event not found for this workflow".to_string(),
                        details: None,
                    }),
                )
                    .into_response();
            }

            if hil_event.status != newton_types::HilStatus::Pending {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        code: "API-HIL-001".to_string(),
                        category: "ValidationError".to_string(),
                        message: "HIL event already resolved".to_string(),
                        details: None,
                    }),
                )
                    .into_response();
            }

            if let Err((status, error)) = apply_hil_action(&mut hil_event, &action) {
                return (status, Json(error)).into_response();
            }
            let _ = state.events_tx.send(BroadcastEvent::HilEvent {
                instance_id: hil_event.instance_id.clone(),
                event_id,
            });
            (StatusCode::OK, Json(hil_event.clone())).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "API-HIL-001".to_string(),
                category: "ValidationError".to_string(),
                message: "HIL event not found".to_string(),
                details: None,
            }),
        )
            .into_response(),
    }
}

async fn list_operators(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!(state.operators.as_ref()))
}

#[derive(Debug, Serialize)]
struct LegacyChannelInfo {
    name: String,
    message_count: usize,
    oldest_message: Option<String>,
    newest_message: Option<String>,
}

#[derive(Debug, Serialize)]
struct LegacyMessage {
    id: Uuid,
    channel: String,
    content: Value,
    timestamp: String,
}

#[derive(Debug, Deserialize)]
struct LegacyMessagesQuery {
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct LegacySubmitResponse {
    ok: bool,
    event_id: Uuid,
    status: HilStatus,
}

async fn legacy_list_channels_v1(State(state): State<Arc<AppState>>) -> Json<Value> {
    let mut channels: Vec<LegacyChannelInfo> = state
        .instances
        .iter()
        .map(|entry| {
            let workflow_id = entry.value().workflow_id.clone();
            let mut timestamps: Vec<_> = state
                .hil_events
                .iter()
                .filter(|evt| evt.value().channel == workflow_id)
                .map(|evt| evt.value().timestamp)
                .collect();
            timestamps.sort_unstable();

            LegacyChannelInfo {
                name: workflow_id,
                message_count: timestamps.len(),
                oldest_message: timestamps.first().map(|ts| ts.to_rfc3339()),
                newest_message: timestamps.last().map(|ts| ts.to_rfc3339()),
            }
        })
        .collect();
    channels.sort_by(|a, b| a.name.cmp(&b.name));
    Json(json!({ "channels": channels }))
}

async fn legacy_list_channel_messages(
    Path(channel): Path<String>,
    Query(query): Query<LegacyMessagesQuery>,
    State(state): State<Arc<AppState>>,
) -> Json<Vec<LegacyMessage>> {
    let mut messages: Vec<LegacyMessage> = state
        .hil_events
        .iter()
        .filter(|evt| evt.value().channel == channel)
        .map(|entry| {
            let event = entry.value();
            let content = match event.event_type {
                newton_types::HilEventType::Question => json!({
                    "type": "question",
                    "text": event.question,
                    "choices": event.choices,
                    "timeout_seconds": event.timeout_seconds,
                }),
                newton_types::HilEventType::Authorization => json!({
                    "type": "authorization",
                    "action": event.question,
                    "choices": event.choices,
                    "timeout_seconds": event.timeout_seconds,
                }),
            };

            LegacyMessage {
                id: event.event_id,
                channel: event.channel.clone(),
                content,
                timestamp: event.timestamp.to_rfc3339(),
            }
        })
        .collect();

    messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    let limit = query.limit.unwrap_or(50).min(200);
    if messages.len() > limit {
        messages.truncate(limit);
    }
    Json(messages)
}

async fn legacy_submit_message_response(
    Path(event_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
    Json(action): Json<HilAction>,
) -> Response {
    match state.hil_events.get_mut(&event_id) {
        Some(mut hil_event) => {
            if let Err((status, error)) = apply_hil_action(&mut hil_event, &action) {
                return (status, Json(error)).into_response();
            }

            let _ = state.events_tx.send(BroadcastEvent::HilEvent {
                instance_id: hil_event.instance_id.clone(),
                event_id,
            });

            (
                StatusCode::OK,
                Json(LegacySubmitResponse {
                    ok: true,
                    event_id,
                    status: hil_event.status.clone(),
                }),
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "API-HIL-001".to_string(),
                category: "ValidationError".to_string(),
                message: "HIL event not found".to_string(),
                details: None,
            }),
        )
            .into_response(),
    }
}

async fn legacy_list_channels(State(state): State<Arc<AppState>>) -> Json<Value> {
    let channels: Vec<String> = state
        .instances
        .iter()
        .map(|entry| entry.value().workflow_id.clone())
        .collect();
    Json(json!({ "channels": channels }))
}

#[derive(Debug, Deserialize, Clone)]
struct StreamFilters {
    pub instance_id: Option<String>,
    pub node_id: Option<String>,
    #[allow(dead_code)]
    pub event_type: Option<String>,
}

async fn workflow_stream(
    ws: axum::extract::ws::WebSocketUpgrade,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Query(filters): Query<StreamFilters>,
) -> Response {
    if Uuid::parse_str(&id).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "API-STREAM-001".to_string(),
                category: "ValidationError".to_string(),
                message: "Invalid workflow instance ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    ws.on_upgrade(move |socket| handle_workflow_socket(socket, id, state, filters))
}

async fn handle_workflow_socket(
    mut socket: axum::extract::ws::WebSocket,
    instance_id: String,
    state: Arc<AppState>,
    filters: StreamFilters,
) {
    let mut rx = state.events_tx.subscribe();

    while let Ok(event) = rx.recv().await {
        if should_send_event(&event, &instance_id, &filters) {
            if let Ok(json) = serde_json::to_string(&event) {
                if socket
                    .send(axum::extract::ws::Message::Text(json.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}

async fn logs_stream(
    ws: axum::extract::ws::WebSocketUpgrade,
    Path((instance_id, node_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Query(filters): Query<StreamFilters>,
) -> Response {
    if Uuid::parse_str(&instance_id).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "API-STREAM-001".to_string(),
                category: "ValidationError".to_string(),
                message: "Invalid workflow instance ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    ws.on_upgrade(move |socket| handle_logs_socket(socket, instance_id, node_id, state, filters))
}

async fn handle_logs_socket(
    mut socket: axum::extract::ws::WebSocket,
    instance_id: String,
    node_id: String,
    state: Arc<AppState>,
    filters: StreamFilters,
) {
    let mut rx = state.events_tx.subscribe();

    while let Ok(event) = rx.recv().await {
        if should_send_event(&event, &instance_id, &filters) {
            if let BroadcastEvent::LogMessage {
                instance_id: ref evt_inst,
                node_id: ref evt_node,
                ..
            } = event
            {
                if evt_inst == &instance_id && evt_node == &node_id {
                    if let Ok(json) = serde_json::to_string(&event) {
                        if socket
                            .send(axum::extract::ws::Message::Text(json.into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        }
    }
}

async fn workflow_sse(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Query(filters): Query<StreamFilters>,
) -> Response {
    if Uuid::parse_str(&id).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "API-STREAM-001".to_string(),
                category: "ValidationError".to_string(),
                message: "Invalid workflow instance ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    let rx = state.events_tx.subscribe();
    let id_clone = id.clone();
    let filters_clone = filters.clone();

    let stream = async_stream::stream! {
        let mut rx = rx;
        while let Ok(event) = rx.recv().await {
            if should_send_event(&event, &id_clone, &filters_clone) {
                if let Ok(json) = serde_json::to_string(&event) {
                    let sse_event = axum::response::sse::Event::default().data(json);
                    yield Ok::<_, Infallible>(sse_event);
                }
            }
        }
    };

    Sse::new(stream)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(10))
                .text("keepalive"),
        )
        .into_response()
}

fn apply_hil_action(
    hil_event: &mut HilEvent,
    action: &HilAction,
) -> Result<(), (StatusCode, ApiError)> {
    match action.response_type.as_str() {
        "text" | "authorization_approved" | "authorization_denied" | "timeout" | "cancelled" => {}
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                ApiError {
                    code: "API-HIL-002".to_string(),
                    category: "ValidationError".to_string(),
                    message: "Invalid response type for HIL event kind".to_string(),
                    details: None,
                },
            ))
        }
    }

    if action.response_type == "text" && action.answer.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            ApiError {
                code: "API-HIL-003".to_string(),
                category: "ValidationError".to_string(),
                message: "Missing answer field for text response type".to_string(),
                details: None,
            },
        ));
    }

    hil_event.status = match action.response_type.as_str() {
        "timeout" => HilStatus::TimedOut,
        "cancelled" => HilStatus::Cancelled,
        _ => HilStatus::Resolved,
    };
    Ok(())
}

fn should_send_event(event: &BroadcastEvent, instance_id: &str, filters: &StreamFilters) -> bool {
    if let Some(ref filter_inst) = filters.instance_id {
        if filter_inst != instance_id {
            return false;
        }
    }

    match event {
        BroadcastEvent::WorkflowInstanceUpdated {
            instance_id: ref evt_id,
        } => evt_id == instance_id,
        BroadcastEvent::NodeStateChanged {
            instance_id: ref evt_id,
            node_id: ref evt_node,
        } => {
            if evt_id != instance_id {
                return false;
            }
            if let Some(ref filter_node) = filters.node_id {
                filter_node == evt_node
            } else {
                true
            }
        }
        BroadcastEvent::LogMessage {
            instance_id: ref evt_id,
            node_id: ref evt_node,
            ..
        } => {
            if evt_id != instance_id {
                return false;
            }
            if let Some(ref filter_node) = filters.node_id {
                filter_node == evt_node
            } else {
                true
            }
        }
        BroadcastEvent::HilEvent {
            instance_id: ref evt_id,
            ..
        } => evt_id == instance_id,
    }
}
