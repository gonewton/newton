use crate::api::state::AppState;
use axum::{
    extract::Path,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use newton_types::{ApiError, BroadcastEvent, HilAction, HilEvent, HilStatus};
use std::sync::Arc;

/// Routes for the human-in-the-loop (HIL) API resource.
pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/hil/instances", get(list_hil_instances))
        .route("/api/hil/workflows/{id}", get(list_hil_events))
        .route(
            "/api/hil/workflows/{id}/{event_id}/action",
            post(submit_hil_action),
        )
        .with_state(state)
}

/// List distinct workflow instance IDs that currently have HIL events.
async fn list_hil_instances(State(state): State<Arc<AppState>>) -> Json<Vec<String>> {
    let mut instance_ids: Vec<String> = state
        .hil_events
        .iter()
        .map(|entry| entry.value().instance_id.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    instance_ids.sort();
    Json(instance_ids)
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
    Path((instance_id, event_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(action): Json<HilAction>,
) -> Response {
    match state.hil_events.get_mut(&event_id) {
        Some(mut hil_event) => {
            if hil_event.instance_id != instance_id {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ApiError {
                        code: "ERR_NOT_FOUND".to_string(),
                        category: "resource".to_string(),
                        message: "HIL event not found for this workflow".to_string(),
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
                event_id: event_id.clone(),
            });
            (StatusCode::OK, Json(hil_event.clone())).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "ERR_NOT_FOUND".to_string(),
                category: "resource".to_string(),
                message: "HIL event not found".to_string(),
                details: None,
            }),
        )
            .into_response(),
    }
}

pub(crate) fn apply_hil_action(
    hil_event: &mut HilEvent,
    action: &HilAction,
) -> Result<(), (StatusCode, ApiError)> {
    match action.response_type.as_str() {
        "text" | "authorization_approved" | "authorization_denied" | "timeout" | "cancelled" => {}
        _ => {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                ApiError {
                    code: "ERR_VALIDATION".to_string(),
                    category: "validation".to_string(),
                    message: "Invalid response type for HIL event kind".to_string(),
                    details: None,
                },
            ))
        }
    }

    if action.response_type == "text" && action.answer.is_none() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            ApiError {
                code: "ERR_VALIDATION".to_string(),
                category: "validation".to_string(),
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
