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

fn no_backend() -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(newton_backend::err_internal("Backend store not configured")),
    )
}

async fn list_repos(State(state): State<Arc<AppState>>) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.list_repos().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

async fn list_repo_dependencies(State(state): State<Arc<AppState>>) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.list_repo_dependencies().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

async fn list_module_dependencies(State(state): State<Arc<AppState>>) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.list_module_dependencies().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

async fn create_module_dependency(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateModuleDependencyBody>,
) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.create_module_dependency(body).await {
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
struct SavedViewsQuery {
    kind: Option<String>,
}

async fn list_saved_views(
    Query(query): Query<SavedViewsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.list_saved_views(query.kind).await {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}
