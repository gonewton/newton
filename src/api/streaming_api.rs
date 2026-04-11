use crate::api::state::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query,
    },
    http::StatusCode,
    response::{IntoResponse, Response, Sse},
    routing::get,
    Json, Router,
};
use newton_types::{ApiError, BroadcastEvent};
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize, Clone)]
pub struct StreamFilters {
    pub instance_id: Option<String>,
    pub node_id: Option<String>,
    pub event_type: Option<String>,
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/stream/workflow/{id}/ws", get(workflow_stream))
        .route("/api/stream/logs/{id}/{node_id}/ws", get(logs_stream))
        .route("/api/stream/workflow/{id}/sse", get(workflow_sse))
        .with_state(state)
}

async fn workflow_stream(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
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
    mut socket: WebSocket,
    instance_id: String,
    state: Arc<AppState>,
    filters: StreamFilters,
) {
    let mut rx = state.events_tx.subscribe();

    while let Ok(event) = rx.recv().await {
        if should_send_event(&event, &instance_id, &filters) {
            if let Ok(json) = serde_json::to_string(&event) {
                if socket.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn logs_stream(
    ws: WebSocketUpgrade,
    Path((instance_id, node_id)): Path<(String, String)>,
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
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
    mut socket: WebSocket,
    instance_id: String,
    node_id: String,
    state: Arc<AppState>,
    filters: StreamFilters,
) {
    let mut rx = state.events_tx.subscribe();

    while let Ok(event) = rx.recv().await {
        if should_send_event(&event, &instance_id, &filters) {
            if let newton_types::BroadcastEvent::LogMessage {
                instance_id: ref evt_inst,
                node_id: ref evt_node,
                ..
            } = event
            {
                if evt_inst == &instance_id && evt_node == &node_id {
                    if let Ok(json) = serde_json::to_string(&event) {
                        if socket.send(Message::Text(json.into())).await.is_err() {
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
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
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

fn should_send_event(event: &BroadcastEvent, instance_id: &str, filters: &StreamFilters) -> bool {
    if let Some(ref filter_inst) = filters.instance_id {
        if filter_inst != instance_id {
            return false;
        }
    }

    if let Some(ref filter_type) = filters.event_type {
        let event_type = match event {
            BroadcastEvent::WorkflowInstanceUpdated { .. } => "workflowInstanceUpdated",
            BroadcastEvent::NodeStateChanged { .. } => "nodeStateChanged",
            BroadcastEvent::LogMessage { .. } => "logMessage",
            BroadcastEvent::HilEvent { .. } => "hilEvent",
        };

        if filter_type != event_type {
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
