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

fn no_backend() -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(newton_backend::err_internal("Backend store not configured")),
    )
}

async fn get_persistence(Path(key): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    if key.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(newton_backend::err_validation("Key must not be empty")),
        )
            .into_response();
    }
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.get_persistence(&key).await {
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

async fn put_persistence(
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
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.put_persistence(&key, body).await {
        Ok(()) => (StatusCode::OK, Json(json!({"ok": true}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

async fn delete_persistence(
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
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.delete_persistence(&key).await {
        Ok(()) => (StatusCode::OK, Json(json!({"ok": true}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}
