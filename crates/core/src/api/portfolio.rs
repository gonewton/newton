use crate::api::ok_json;
use crate::api::state::AppState;
use axum::{
    extract::{Query, State},
    response::Response,
    routing::get,
    Router,
};
use newton_types::ApiError;
use serde::Deserialize;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/repo-dependencies", get(list_repo_dependencies))
        .route("/saved-views", get(list_saved_views))
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/repo-dependencies",
    tag = "portfolio",
    responses(
        (status = 200, description = "Repository dependency list", body = [newton_types::RepoDependencyItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_repo_dependencies(State(state): State<Arc<AppState>>) -> Response {
    ok_json(state.backend.list_repo_dependencies().await)
}

#[derive(Debug, Deserialize)]
pub(crate) struct SavedViewsQuery {
    kind: Option<String>,
}

#[utoipa::path(
    get,
    path = "/saved-views",
    tag = "portfolio",
    params(("kind" = Option<String>, Query, description = "Optional saved-view kind filter")),
    responses(
        (status = 200, description = "Saved views", body = serde_json::Value),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_saved_views(
    Query(query): Query<SavedViewsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    ok_json(state.backend.list_saved_views(query.kind).await)
}
