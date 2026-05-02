use crate::api::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{delete, get, put},
    Router,
};
use newton_types::ApiError;
use serde_json::json;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/persistence/{key}", get(get_persistence))
        .route("/api/persistence/{key}", put(put_persistence))
        .route("/api/persistence/{key}", delete(delete_persistence))
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/api/persistence/{key}",
    tag = "persistence",
    params(("key" = String, Path, description = "Persistence key")),
    responses(
        (status = 200, description = "Persisted JSON value", body = serde_json::Value),
        (status = 404, description = "Key not found", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn get_persistence(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    if key.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(newton_backend::err_validation("Key must not be empty")),
        )
            .into_response();
    }
    match state.backend.get_persistence(&key).await {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(e) => {
            let status = match e.code.as_str() {
                "ERR_NOT_FOUND" => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(e)).into_response()
        }
    }
}

#[utoipa::path(
    put,
    path = "/api/persistence/{key}",
    tag = "persistence",
    params(("key" = String, Path, description = "Persistence key")),
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Persistence upsert result", body = serde_json::Value),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn put_persistence(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    if key.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(newton_backend::err_validation("Key must not be empty")),
        )
            .into_response();
    }
    match state.backend.put_persistence(&key, body).await {
        Ok(()) => (StatusCode::OK, Json(json!({"ok": true}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/api/persistence/{key}",
    tag = "persistence",
    params(("key" = String, Path, description = "Persistence key")),
    responses(
        (status = 200, description = "Persistence delete result", body = serde_json::Value),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn delete_persistence(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    if key.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(newton_backend::err_validation("Key must not be empty")),
        )
            .into_response();
    }
    match state.backend.delete_persistence(&key).await {
        Ok(()) => (StatusCode::OK, Json(json!({"ok": true}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}
