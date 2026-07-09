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

/// Environment variable that must be set to `1` for `POST /testing/reset` to be
/// reachable. Deliberately not documented in the OpenAPI surface (see
/// `crates/core/src/api/openapi.rs`): this endpoint wipes the entire backend
/// store and is intended only for test harnesses that explicitly opt in.
const NEWTON_ENABLE_TESTING_RESET: &str = "NEWTON_ENABLE_TESTING_RESET";

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/testing/reset", post(reset_testing))
        .with_state(state)
}

/// Wipes the entire backend store. Gated behind `NEWTON_ENABLE_TESTING_RESET=1`
/// (absent or any other value ⇒ 403) and deliberately excluded from the
/// generated OpenAPI document — this is a test-harness-only maintenance
/// endpoint, not part of the public API surface.
pub(crate) async fn reset_testing(State(state): State<Arc<AppState>>) -> Response {
    if std::env::var(NEWTON_ENABLE_TESTING_RESET).as_deref() != Ok("1") {
        return (
            StatusCode::FORBIDDEN,
            Json(newton_types::err_testing_reset_disabled(&format!(
                "Testing reset is disabled; set {NEWTON_ENABLE_TESTING_RESET}=1 to enable this endpoint"
            ))),
        )
            .into_response();
    }

    match state.backend.reset().await {
        Ok(()) => (StatusCode::OK, Json(json!({"ok": true}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}
