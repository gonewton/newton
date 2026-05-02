use crate::api::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use newton_types::ApiError;
use serde::Deserialize;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/plans", get(list_plans))
        .route("/api/plans/{id}", get(get_plan))
        .route("/api/plans/{id}/approve", post(approve_plan))
        .route("/api/plans/{id}/reject", post(reject_plan))
        .route("/api/executions", get(list_executions))
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/api/plans",
    tag = "plans",
    responses(
        (status = 200, description = "Plan list", body = [newton_backend::PlanItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_plans(State(state): State<Arc<AppState>>) -> Response {
    match state.backend.list_plans().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/plans/{id}",
    tag = "plans",
    params(("id" = String, Path, description = "Plan id")),
    responses(
        (status = 200, description = "Plan detail", body = newton_backend::PlanDetail),
        (status = 404, description = "Plan not found", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn get_plan(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.get_plan(&id).await {
        Ok(detail) => (StatusCode::OK, Json(detail)).into_response(),
        Err(e) => {
            let status = match e.code.as_str() {
                "ERR_NOT_FOUND" => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(e)).into_response()
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/plans/{id}/approve",
    tag = "plans",
    params(("id" = String, Path, description = "Plan id")),
    responses(
        (status = 200, description = "Approved plan", body = newton_backend::PlanItem),
        (status = 404, description = "Plan not found", body = ApiError),
        (status = 409, description = "Plan state conflict", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn approve_plan(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.approve_plan(&id).await {
        Ok(approved) => {
            let _ = state
                .events_tx
                .send(newton_types::BroadcastEvent::PlanUpdate {
                    plan_id: id.clone(),
                });
            let _ = state
                .events_tx
                .send(newton_types::BroadcastEvent::ExecutionUpdate {
                    execution_id: approved.execution_id.clone(),
                    plan_id: Some(id.clone()),
                    status: "running".to_string(),
                    created_at: approved.created_at.clone(),
                });
            (StatusCode::OK, Json(approved.plan)).into_response()
        }
        Err(e) => {
            let status = match e.code.as_str() {
                "ERR_NOT_FOUND" => StatusCode::NOT_FOUND,
                "ERR_CONFLICT" => StatusCode::CONFLICT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(e)).into_response()
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/plans/{id}/reject",
    tag = "plans",
    params(("id" = String, Path, description = "Plan id")),
    responses(
        (status = 200, description = "Rejected plan", body = newton_backend::PlanItem),
        (status = 404, description = "Plan not found", body = ApiError),
        (status = 409, description = "Plan state conflict", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn reject_plan(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.reject_plan(&id).await {
        Ok(plan) => {
            let _ = state
                .events_tx
                .send(newton_types::BroadcastEvent::PlanUpdate {
                    plan_id: id.clone(),
                });
            (StatusCode::OK, Json(plan)).into_response()
        }
        Err(e) => {
            let status = match e.code.as_str() {
                "ERR_NOT_FOUND" => StatusCode::NOT_FOUND,
                "ERR_CONFLICT" => StatusCode::CONFLICT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(e)).into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecutionsQuery {
    plan_id: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/executions",
    tag = "executions",
    params(("planId" = Option<String>, Query, description = "Optional plan id filter")),
    responses(
        (status = 200, description = "Execution list", body = [newton_backend::ExecutionItem]),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_executions(
    Query(query): Query<ExecutionsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.backend.list_executions(query.plan_id).await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}
