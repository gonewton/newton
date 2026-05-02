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

#[utoipa::path(
    get,
    path = "/api/requests",
    tag = "requests",
    responses(
        (status = 200, description = "Request list", body = [newton_backend::RequestItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_requests(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_requests().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/api/requests",
    tag = "requests",
    request_body = CreateRequestBody,
    responses(
        (status = 201, description = "Created request", body = newton_backend::RequestItem),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn create_request(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateRequestBody>,
) -> Response {
    match state.backend.create_request(body).await {
        Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}
