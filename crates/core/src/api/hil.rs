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
        .route("/hil/instances", get(list_hil_instances))
        .route("/hil/workflows/{id}", get(list_hil_events))
        .route(
            "/hil/workflows/{id}/{event_id}/action",
            post(submit_hil_action),
        )
        .with_state(state)
}

fn map_store_err(_e: ApiError) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            code: "ERR_INTERNAL".to_string(),
            category: "internal".to_string(),
            message: "Internal storage error".to_string(),
            details: None,
        }),
    )
        .into_response()
}

/// List distinct workflow instance IDs that currently have HIL events.
#[utoipa::path(
    get,
    path = "/hil/instances",
    tag = "hil",
    responses((status = 200, description = "HIL workflow instance ids", body = [String]))
)]
pub(crate) async fn list_hil_instances(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_hil_instances().await {
        Ok(ids) => (StatusCode::OK, Json(ids)).into_response(),
        Err(e) => map_store_err(e),
    }
}

#[utoipa::path(
    get,
    path = "/hil/workflows/{id}",
    tag = "hil",
    params(("id" = String, Path, description = "Workflow instance id")),
    responses((status = 200, description = "HIL event list", body = [HilEvent]))
)]
pub(crate) async fn list_hil_events(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.list_hil_events_for_instance(&id).await {
        Ok(events) => (StatusCode::OK, Json(events)).into_response(),
        Err(e) => map_store_err(e),
    }
}

#[utoipa::path(
    post,
    path = "/hil/workflows/{id}/{event_id}/action",
    tag = "hil",
    params(
        ("id" = String, Path, description = "Workflow instance id"),
        ("event_id" = String, Path, description = "HIL event id")
    ),
    request_body = HilAction,
    responses(
        (status = 200, description = "Resolved HIL event", body = HilEvent),
        (status = 404, description = "HIL event not found", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError)
    )
)]
pub(crate) async fn submit_hil_action(
    Path((instance_id, event_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(action): Json<HilAction>,
) -> Response {
    // Fetch the HIL event
    let hil_event = match state.backend.get_hil_event(&event_id).await {
        Ok(e) => e,
        Err(e) if e.code == "ERR_NOT_FOUND" => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    code: "ERR_NOT_FOUND".to_string(),
                    category: "resource".to_string(),
                    message: "HIL event not found".to_string(),
                    details: None,
                }),
            )
                .into_response()
        }
        Err(e) => return map_store_err(e),
    };

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

    let new_status = match apply_hil_action_status(&action) {
        Ok(s) => s,
        Err((status, error)) => return (status, Json(error)).into_response(),
    };

    // Persist updated status first, then broadcast
    let updated = match state
        .backend
        .update_hil_event_status(&event_id, new_status)
        .await
    {
        Ok(e) => e,
        Err(e) => return map_store_err(e),
    };

    let _ = state.events_tx.send(BroadcastEvent::HilEvent {
        instance_id: updated.instance_id.clone(),
        event_id: event_id.clone(),
    });

    (StatusCode::OK, Json(updated)).into_response()
}

#[allow(dead_code)]
pub(crate) fn apply_hil_action(
    hil_event: &mut HilEvent,
    action: &HilAction,
) -> Result<(), (StatusCode, ApiError)> {
    hil_event.status = apply_hil_action_status(action)?;
    Ok(())
}

fn apply_hil_action_status(action: &HilAction) -> Result<HilStatus, (StatusCode, ApiError)> {
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

    Ok(match action.response_type.as_str() {
        "timeout" => HilStatus::TimedOut,
        "cancelled" => HilStatus::Cancelled,
        _ => HilStatus::Resolved,
    })
}
