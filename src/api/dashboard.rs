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

fn no_backend() -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(newton_backend::err_internal("Backend store not configured")),
    )
}

async fn list_products(State(state): State<Arc<AppState>>) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.list_products().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

async fn list_components(State(state): State<Arc<AppState>>) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.list_components().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

async fn list_pending_approvals(State(state): State<Arc<AppState>>) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.list_pending_approvals().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

async fn list_regressions(State(state): State<Arc<AppState>>) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.list_regressions().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

async fn list_indicators(State(state): State<Arc<AppState>>) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.list_indicators().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct RecentActionsQuery {
    limit: Option<u32>,
}

async fn list_recent_actions(
    Query(query): Query<RecentActionsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    let limit = query.limit.unwrap_or(20).max(1);
    match store.list_recent_actions(limit).await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}
