use crate::api::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, patch},
    Router,
};
use newton_backend::PatchOpportunityBody;
use newton_types::ApiError;
use serde::Deserialize;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/opportunities", get(list_opportunities))
        .route("/api/opportunities/{id}", patch(patch_opportunity))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpportunityQuery {
    status: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/opportunities",
    tag = "opportunities",
    params(("status" = Option<String>, Query, description = "Optional opportunity status filter")),
    responses(
        (status = 200, description = "Opportunity list", body = [newton_backend::OpportunityItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_opportunities(
    Query(query): Query<OpportunityQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.list_opportunities(query.status).await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[utoipa::path(
    patch,
    path = "/api/opportunities/{id}",
    tag = "opportunities",
    params(("id" = String, Path, description = "Opportunity id")),
    request_body = PatchOpportunityBody,
    responses(
        (status = 200, description = "Updated opportunity", body = newton_backend::OpportunityItem),
        (status = 404, description = "Opportunity not found", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn patch_opportunity(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<PatchOpportunityBody>,
) -> Response {
    match state.backend.patch_opportunity(&id, body).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => {
            let status = match e.code.as_str() {
                "ERR_NOT_FOUND" => StatusCode::NOT_FOUND,
                "ERR_VALIDATION" => StatusCode::UNPROCESSABLE_ENTITY,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(e)).into_response()
        }
    }
}
