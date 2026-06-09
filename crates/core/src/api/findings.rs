use crate::api::state::AppState;
use crate::api::{created_json, ok_json};
use axum::{
    extract::Json,
    extract::{Path, Query, State},
    response::Response,
    routing::get,
    Router,
};
use newton_backend::{CreateFindingBody, PatchFindingBody};
use newton_types::ApiError;
use serde::Deserialize;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/findings", get(list_findings).post(create_finding))
        .route("/findings/{id}", get(get_finding).patch(patch_finding))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub(crate) struct FindingQuery {
    status: Option<String>,
    scope: Option<String>,
    scope_id: Option<String>,
}

#[utoipa::path(
    get,
    path = "/findings",
    tag = "findings",
    params(
        ("status" = Option<String>, Query, description = "Optional finding status filter"),
        ("scope" = Option<String>, Query, description = "Scope kind: component | repo | module"),
        ("scope_id" = Option<String>, Query, description = "Optional scope entity id filter"),
    ),
    responses(
        (status = 200, description = "Finding list", body = [newton_backend::FindingItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_findings(
    Query(query): Query<FindingQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    ok_json(
        state
            .backend
            .list_findings(query.status, query.scope, query.scope_id)
            .await,
    )
}

#[utoipa::path(
    post,
    path = "/findings",
    tag = "findings",
    request_body = CreateFindingBody,
    responses(
        (status = 201, description = "Created or upserted finding", body = newton_backend::FindingItem),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn create_finding(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateFindingBody>,
) -> Response {
    created_json(state.backend.create_finding(body).await)
}

#[utoipa::path(
    get,
    path = "/findings/{id}",
    tag = "findings",
    params(("id" = String, Path, description = "Finding id")),
    responses(
        (status = 200, description = "Finding detail", body = newton_backend::FindingItem),
        (status = 404, description = "Finding not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn get_finding(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    ok_json(state.backend.get_finding(&id).await)
}

#[utoipa::path(
    patch,
    path = "/findings/{id}",
    tag = "findings",
    params(("id" = String, Path, description = "Finding id")),
    request_body = PatchFindingBody,
    responses(
        (status = 200, description = "Updated finding", body = newton_backend::FindingItem),
        (status = 404, description = "Finding not found", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn patch_finding(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<PatchFindingBody>,
) -> Response {
    ok_json(state.backend.patch_finding(&id, body).await)
}
