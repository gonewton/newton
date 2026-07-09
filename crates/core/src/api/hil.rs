use crate::api::state::AppState;
use axum::{
    extract::Path,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use newton_types::{ApiError, BroadcastEvent, HilAction, HilEvent, HilEventType, HilStatus};
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
        (status = 409, description = "HIL event is not pending (already resolved, timed out, or cancelled)", body = ApiError),
        (status = 422, description = "response_type is invalid for the event's kind, or a required field (e.g. answer) is missing", body = ApiError)
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

    // NEWTON audit B15: an event can only be resolved once. Check the
    // event's current status *before* doing any further validation so a
    // stale/duplicate submission gets a 409 regardless of what response_type
    // it carries. `update_hil_event_status` (see newton-types::BackendStore)
    // is a blind `UPDATE ... WHERE eventId = ?` with no `WHERE status =
    // 'pending'` guard — there is no compare-and-swap primitive to route
    // through here. This check-then-set has a narrow but real TOCTOU window:
    // two concurrent submissions for the same pending event can both read
    // Pending here and both proceed to the update below (last write wins,
    // both requests observe 200). Closing that residual race requires either
    // extending `BackendStore::update_hil_event_status` to a conditional
    // `update_hil_event_status_if_pending` (touches newton-types + the
    // sqlite backend impl) or a per-event mutex in `AppState` — out of scope
    // for this handler-only fix.
    if hil_event.status != HilStatus::Pending {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                code: "ERR_CONFLICT".to_string(),
                category: "resource".to_string(),
                message: format!(
                    "HIL event is not pending (current status: {:?}); it has already been resolved",
                    hil_event.status
                ),
                details: None,
            }),
        )
            .into_response();
    }

    let new_status = match apply_hil_action_status(&hil_event.event_type, &action) {
        Ok(s) => s,
        Err((status, error)) => return (status, Json(error)).into_response(),
    };

    // Persist updated status first, then broadcast. Narrowest possible
    // window between the pending-check above and this write: no further
    // async/IO work happens in between.
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
    hil_event.status = apply_hil_action_status(&hil_event.event_type, action)?;
    Ok(())
}

/// The `response_type` values accepted for a given HIL event kind.
///
/// Enforced mapping (derived from `newton_types::{HilEvent, HilEventType,
/// HilStatus}` and the ailoop-core `MessageContent`/`ResponseType` vocabulary
/// used by `crate::workflow::human::ailoop`):
///
/// - `HilEventType::Question` (ailoop `Decision` requests): `"text"` (the
///   chosen answer/option), plus the two terminal responses that can close
///   out *any* pending event without an answer: `"timeout"`, `"cancelled"`.
///   `"authorization_approved"`/`"authorization_denied"` are rejected —
///   ailoop's own decision-handling code treats those `ResponseType`
///   variants as "unavailable" for a decision.
/// - `HilEventType::Authorization` (ailoop `Authorization` requests):
///   `"authorization_approved"`, `"authorization_denied"`, plus the same
///   two terminal responses `"timeout"`, `"cancelled"`. `"text"` is
///   rejected — ailoop's authorization-handling code treats `Text` as
///   "unavailable" for an authorization.
fn allowed_response_types(kind: &HilEventType) -> &'static [&'static str] {
    match kind {
        HilEventType::Question => &["text", "timeout", "cancelled"],
        HilEventType::Authorization => &[
            "authorization_approved",
            "authorization_denied",
            "timeout",
            "cancelled",
        ],
    }
}

fn apply_hil_action_status(
    event_type: &HilEventType,
    action: &HilAction,
) -> Result<HilStatus, (StatusCode, ApiError)> {
    let allowed = allowed_response_types(event_type);
    if !allowed.contains(&action.response_type.as_str()) {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            ApiError {
                code: "ERR_VALIDATION".to_string(),
                category: "validation".to_string(),
                message: format!(
                    "response_type '{}' is not valid for a {event_type:?} HIL event; allowed values: {allowed:?}",
                    action.response_type
                ),
                details: None,
            },
        ));
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
