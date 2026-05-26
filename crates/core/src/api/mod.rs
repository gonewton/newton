pub mod catalog;
pub mod dashboard;
pub mod hil;
pub mod magic_tools;
pub mod openapi;
pub mod operators;
pub mod opportunities;
pub mod persistence;
pub mod plans;
pub mod portfolio;
pub mod requests;
pub mod state;
pub mod streaming;
pub mod testing_reset;
pub mod workflow_files;
pub mod workflows;

use crate::api::state::AppState;
use axum::Router;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};

// lockstep: axum major version MUST match cli-framework (both 0.8)
pub fn api_v1_router(state: AppState) -> Router {
    let arc_state = Arc::new(state);
    Router::new()
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
        .merge(catalog::routes(arc_state.clone()))
        .merge(testing_reset::routes(arc_state.clone()))
        .merge(workflow_files::routes(arc_state.clone()))
        .merge(aikit_magictool::router(magic_tools::build_state()))
}

pub fn static_ui_router(dir: PathBuf) -> Router {
    Router::new().fallback_service(
        ServeDir::new(&dir).not_found_service(ServeFile::new(dir.join("index.html"))),
    )
}

pub fn openapi_json() -> serde_json::Value {
    use utoipa::OpenApi;
    serde_json::to_value(openapi::ApiDoc::openapi()).expect("OpenAPI doc serialization failed")
}
