use crate::api::hil::apply_hil_action;
use crate::api::state::AppState;
use axum::{
    extract::Path,
    extract::Query,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use newton_types::{ApiError, BroadcastEvent, HilAction, HilStatus};
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

/// Routes for legacy channel/message endpoints kept for backward compatibility.
pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/channels", get(legacy_list_channels_v1))
        .route(
            "/api/channels/{channel}/messages",
            get(legacy_list_channel_messages),
        )
        .route(
            "/api/v1/messages/{event_id}/response",
            post(legacy_submit_message_response),
        )
        .route("/channels", get(legacy_list_channels))
        .with_state(state)
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
