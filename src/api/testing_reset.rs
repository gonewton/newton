use crate::api::state::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::post,
    Router,
};
use newton_types::ApiError;
use serde_json::json;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/testing/reset", post(reset_testing))
        .with_state(state)
}

#[utoipa::path(
    post,
    path = "/api/testing/reset",
    tag = "testing",
    responses(
        (status = 200, description = "Reset result", body = serde_json::Value),
        (status = 403, description = "Forbidden in production", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn reset_testing(State(state): State<Arc<AppState>>) -> Response {
    if std::env::var("NEWTON_ENV").as_deref() == Ok("production") {
        return (
            StatusCode::FORBIDDEN,
            Json(newton_backend::err_forbidden_in_prod(
                "Testing reset is not available in production",
            )),
        )
            .into_response();
    }

    match state.backend.reset().await {
        Ok(()) => (StatusCode::OK, Json(json!({"ok": true}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}
