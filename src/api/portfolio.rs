use crate::api::state::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use newton_backend::CreateModuleDependencyBody;
use newton_types::ApiError;
use serde::Deserialize;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/repos", get(list_repos))
        .route("/api/repo-dependencies", get(list_repo_dependencies))
        .route("/api/module-dependencies", get(list_module_dependencies))
        .route("/api/module-dependencies", post(create_module_dependency))
        .route("/api/saved-views", get(list_saved_views))
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/api/repos",
    tag = "portfolio",
    responses(
        (status = 200, description = "Repository list", body = [newton_backend::RepoItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_repos(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_repos().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/repo-dependencies",
    tag = "portfolio",
    responses(
        (status = 200, description = "Repository dependency list", body = [newton_backend::RepoDependencyItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_repo_dependencies(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_repo_dependencies().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/module-dependencies",
    tag = "portfolio",
    responses(
        (status = 200, description = "Module dependency list", body = [newton_backend::ModuleDependencyItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_module_dependencies(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_module_dependencies().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/api/module-dependencies",
    tag = "portfolio",
    request_body = CreateModuleDependencyBody,
    responses(
        (status = 201, description = "Created module dependency", body = newton_backend::ModuleDependencyItem),
        (status = 404, description = "Module not found", body = ApiError),
        (status = 409, description = "Dependency conflict", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn create_module_dependency(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateModuleDependencyBody>,
) -> Response {
    match state.backend.create_module_dependency(body).await {
        Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
        Err(e) => {
            let status = match e.code.as_str() {
                "ERR_NOT_FOUND" => StatusCode::NOT_FOUND,
                "ERR_CONFLICT" => StatusCode::CONFLICT,
                "ERR_VALIDATION" => StatusCode::UNPROCESSABLE_ENTITY,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(e)).into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct SavedViewsQuery {
    kind: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/saved-views",
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
    match state.backend.list_saved_views(query.kind).await {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}
