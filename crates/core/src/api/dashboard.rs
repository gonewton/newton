use crate::api::state::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use newton_types::ApiError;
use serde::Deserialize;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/products", get(list_products))
        .route("/api/components", get(list_components))
        .route("/api/pending-approvals", get(list_pending_approvals))
        .route("/api/regressions", get(list_regressions))
        .route("/api/indicators", get(list_indicators))
        .route("/api/recent-actions", get(list_recent_actions))
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/api/products",
    tag = "dashboard",
    responses(
        (status = 200, description = "Product list", body = [newton_backend::ProductItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_products(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_products().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/components",
    tag = "dashboard",
    responses(
        (status = 200, description = "Component list", body = [newton_backend::ComponentItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_components(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_components().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/pending-approvals",
    tag = "dashboard",
    responses(
        (status = 200, description = "Pending approval list", body = [newton_backend::PendingApprovalItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_pending_approvals(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_pending_approvals().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/regressions",
    tag = "dashboard",
    responses(
        (status = 200, description = "Regression list", body = [newton_backend::RegressionItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_regressions(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_regressions().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/indicators",
    tag = "dashboard",
    responses(
        (status = 200, description = "Indicator list", body = [newton_backend::IndicatorItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_indicators(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_indicators().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct RecentActionsQuery {
    limit: Option<u32>,
}

#[utoipa::path(
    get,
    path = "/api/recent-actions",
    tag = "dashboard",
    params(("limit" = Option<u32>, Query, description = "Maximum number of recent actions")),
    responses(
        (status = 200, description = "Recent action list", body = [newton_backend::RecentActionItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_recent_actions(
    Query(query): Query<RecentActionsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let limit = query.limit.unwrap_or(20).max(1);
    match state.backend.list_recent_actions(limit).await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}
