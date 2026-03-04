use crate::api::state::AppState;
use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use newton_types::{ApiError, HilAction, HilEvent};
use std::sync::Arc;
use uuid::Uuid;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/hil/workflows/{id}", get(list_hil_events))
        .route(
            "/api/hil/workflows/{id}/{event_id}/action",
            axum::routing::post(submit_hil_action),
        )
        .with_state(state)
}

async fn list_hil_events(
    Path(id): Path<String>,
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Response {
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
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
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

            match action.response_type.as_str() {
                "text" | "authorization_approved" | "authorization_denied" => {}
                _ => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(ApiError {
                            code: "API-HIL-002".to_string(),
                            category: "ValidationError".to_string(),
                            message: "Invalid response type for HIL event kind".to_string(),
                            details: None,
                        }),
                    )
                        .into_response()
                }
            }

            if action.response_type == "text" && action.answer.is_none() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        code: "API-HIL-003".to_string(),
                        category: "ValidationError".to_string(),
                        message: "Missing answer field for text response type".to_string(),
                        details: None,
                    }),
                )
                    .into_response();
            }

            hil_event.status = newton_types::HilStatus::Resolved;
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
