use crate::api::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, patch},
    Router,
};
use newton_backend::PatchOpportunityBody;
use newton_types::ApiError;
use serde::Deserialize;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/opportunities", get(list_opportunities))
        .route("/api/opportunities/{id}", patch(patch_opportunity))
        .with_state(state)
}

fn no_backend() -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(newton_backend::err_internal("Backend store not configured")),
    )
}

#[derive(Debug, Deserialize)]
struct OpportunityQuery {
    status: Option<String>,
}

async fn list_opportunities(
    Query(query): Query<OpportunityQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.list_opportunities(query.status).await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

async fn patch_opportunity(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<PatchOpportunityBody>,
) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.patch_opportunity(&id, body).await {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => {
            let status = match e.code.as_str() {
                "ERR_NOT_FOUND" => StatusCode::NOT_FOUND,
                "ERR_VALIDATION" => StatusCode::UNPROCESSABLE_ENTITY,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(e)).into_response()
        }
    }
}
