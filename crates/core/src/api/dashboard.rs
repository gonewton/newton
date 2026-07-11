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
        .route("/pending-approvals", get(list_pending_approvals))
        .route("/regressions", get(list_regressions))
        .route("/recent-actions", get(list_recent_actions))
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/pending-approvals",
    tag = "dashboard",
    responses(
        (status = 200, description = "Pending approval list", body = [newton_types::PendingApprovalItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_pending_approvals(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_pending_approvals().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

/// Query params for the previously-unbounded `/regressions` list endpoint
/// (audit S12): `limit` defaults to 100 and is hard-capped at 1000; `offset`
/// defaults to 0. `u32` deserialization already rejects negative values.
#[derive(Debug, Deserialize)]
pub(crate) struct RegressionsQuery {
    limit: Option<u32>,
    offset: Option<u32>,
}

#[utoipa::path(
    get,
    path = "/regressions",
    tag = "dashboard",
    params(
        ("limit" = Option<u32>, Query, description = "Max rows to return (default 100, hard cap 1000)"),
        ("offset" = Option<u32>, Query, description = "Rows to skip (default 0)")
    ),
    responses(
        (status = 200, description = "Regression list", body = [newton_types::RegressionItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_regressions(
    Query(query): Query<RegressionsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let limit = query.limit.unwrap_or(100).min(1000) as usize;
    let offset = query.offset.unwrap_or(0) as usize;
    match state.backend.list_regressions().await {
        Ok(items) => {
            let items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
            (StatusCode::OK, Json(items)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct RecentActionsQuery {
    limit: Option<u32>,
}

#[utoipa::path(
    get,
    path = "/recent-actions",
    tag = "dashboard",
    params(("limit" = Option<u32>, Query, description = "Maximum number of recent actions")),
    responses(
        (status = 200, description = "Recent action list", body = [newton_types::RecentActionItem]),
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
