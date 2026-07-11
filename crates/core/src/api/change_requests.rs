use crate::api::state::AppState;
use crate::api::{created_json, ok_json};
use axum::{
    extract::Json,
    extract::{Path, Query, State},
    response::Response,
    routing::get,
    Router,
};
use newton_types::ApiError;
use newton_types::BroadcastEvent;
use newton_types::{CreateChangeRequestBody, PatchChangeRequestBody};
use serde::Deserialize;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/change-requests",
            get(list_change_requests).post(create_change_request),
        )
        .route(
            "/change-requests/{id}",
            get(get_change_request).patch(patch_change_request),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChangeRequestQuery {
    status: Option<String>,
}

#[utoipa::path(
    get,
    path = "/change-requests",
    tag = "change-requests",
    params(
        ("status" = Option<String>, Query, description = "Optional change request status filter"),
    ),
    responses(
        (status = 200, description = "Change request list", body = [newton_types::ChangeRequestItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_change_requests(
    Query(query): Query<ChangeRequestQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    ok_json(state.backend.list_change_requests(query.status).await)
}

#[utoipa::path(
    post,
    path = "/change-requests",
    tag = "change-requests",
    request_body = CreateChangeRequestBody,
    responses(
        (status = 201, description = "Created change request", body = newton_types::ChangeRequestItem),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn create_change_request(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateChangeRequestBody>,
) -> Response {
    let result = state.backend.create_change_request(body).await;
    if let Ok(ref item) = result {
        let _ = state.events_tx.send(BroadcastEvent::ChangeRequestUpdate {
            change_request_id: item.id.clone(),
        });
    }
    created_json(result)
}

#[utoipa::path(
    get,
    path = "/change-requests/{id}",
    tag = "change-requests",
    params(("id" = String, Path, description = "Change request id")),
    responses(
        (status = 200, description = "Change request detail", body = newton_types::ChangeRequestItem),
        (status = 404, description = "Change request not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn get_change_request(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    ok_json(state.backend.get_change_request(&id).await)
}

#[utoipa::path(
    patch,
    path = "/change-requests/{id}",
    tag = "change-requests",
    params(("id" = String, Path, description = "Change request id")),
    request_body = PatchChangeRequestBody,
    responses(
        (status = 200, description = "Updated change request", body = newton_types::ChangeRequestItem),
        (status = 404, description = "Change request not found", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn patch_change_request(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<PatchChangeRequestBody>,
) -> Response {
    let result = state.backend.patch_change_request(&id, body).await;
    if result.is_ok() {
        let _ = state.events_tx.send(BroadcastEvent::ChangeRequestUpdate {
            change_request_id: id,
        });
    }
    ok_json(result)
}
