pub mod hil;
pub mod operators;
pub mod state;
pub mod streaming;
pub mod workflows;

use crate::api::state::AppState;
use axum::{routing::get, Router};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

/// Build the Axum router for Newton's HTTP API.
///
/// This is a pure composition function that merges per-resource routers, applies
/// shared middleware (CORS), and optionally serves static UI assets.
pub fn create_router(state: AppState, ui_dir: Option<PathBuf>) -> Router {
    let arc_state = Arc::new(state);

    let mut router = Router::new()
        .merge(workflows::routes(arc_state.clone()))
        .merge(hil::routes(arc_state.clone()))
        .merge(streaming::routes(arc_state.clone()))
        .merge(operators::routes(arc_state.clone()))
        .route("/health", get(health_check))
        .layer(CorsLayer::permissive());

    if let Some(ref dir) = ui_dir {
        if dir.exists() {
            router = router.fallback_service(
                ServeDir::new(dir).not_found_service(ServeFile::new(dir.join("index.html"))),
            );
        }
    }

    router
}

async fn health_check() -> axum::response::Json<serde_json::Value> {
    axum::response::Json(json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION")
    }))
}
