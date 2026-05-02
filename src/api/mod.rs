pub mod dashboard;
pub mod hil;
pub mod operators;
pub mod opportunities;
pub mod persistence;
pub mod plans;
pub mod portfolio;
pub mod requests;
pub mod state;
pub mod streaming;
pub mod testing_reset;
pub mod workflows;

use crate::api::state::AppState;
use axum::{routing::get, Router};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

pub fn create_router(state: AppState, ui_dir: Option<PathBuf>) -> Router {
    let arc_state = Arc::new(state);

    let mut router = Router::new()
        .merge(workflows::routes(arc_state.clone()))
        .merge(hil::routes(arc_state.clone()))
        .merge(streaming::routes(arc_state.clone()))
        .merge(operators::routes(arc_state.clone()))
        .merge(dashboard::routes(arc_state.clone()))
        .merge(portfolio::routes(arc_state.clone()))
        .merge(opportunities::routes(arc_state.clone()))
        .merge(requests::routes(arc_state.clone()))
        .merge(plans::routes(arc_state.clone()))
        .merge(persistence::routes(arc_state.clone()))
        .merge(testing_reset::routes(arc_state.clone()))
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
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}
