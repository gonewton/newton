use crate::api::state::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use newton_backend::CreateRequestBody;
use newton_types::ApiError;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/requests", get(list_requests))
        .route("/api/requests", post(create_request))
        .with_state(state)
}

fn no_backend() -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(newton_backend::err_internal("Backend store not configured")),
    )
}

async fn list_requests(State(state): State<Arc<AppState>>) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.list_requests().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

async fn create_request(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRequestBody>,
) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.create_request(body).await {
        Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}
