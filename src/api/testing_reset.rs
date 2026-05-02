use crate::api::state::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::post,
    Router,
};
use serde_json::json;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/testing/reset", post(reset_testing))
        .with_state(state)
}

async fn reset_testing(State(state): State<Arc<AppState>>) -> Response {
    if std::env::var("NEWTON_ENV").as_deref() == Ok("production") {
        return (
            StatusCode::FORBIDDEN,
            Json(newton_backend::err_forbidden_in_prod(
                "Testing reset is not available in production",
            )),
        )
            .into_response();
    }

    let store = match state.backend {
        Some(ref s) => s,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(newton_backend::err_internal("Backend store not configured")),
            )
                .into_response()
        }
    };
    match store.reset().await {
        Ok(()) => (StatusCode::OK, Json(json!({"ok": true}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}
