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

fn no_backend() -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(newton_backend::err_internal("Backend store not configured")),
    )
}

async fn list_plans(State(state): State<Arc<AppState>>) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.list_plans().await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

async fn get_plan(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.get_plan(&id).await {
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

async fn approve_plan(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.approve_plan(&id).await {
        Ok(plan) => {
            let _ = state
                .events_tx
                .send(newton_types::BroadcastEvent::PlanUpdate {
                    plan_id: id.clone(),
                });
            if let Ok(mut executions) = store.list_executions(Some(id.clone())).await {
                executions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                if let Some(execution) = executions.into_iter().next() {
                    let _ = state
                        .events_tx
                        .send(newton_types::BroadcastEvent::ExecutionUpdate {
                            execution_id: execution.instance_id,
                            plan_id: execution.plan_id,
                            status: execution.status,
                            created_at: execution.created_at,
                        });
                }
            }
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

async fn reject_plan(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.reject_plan(&id).await {
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
struct ExecutionsQuery {
    plan_id: Option<String>,
}

async fn list_executions(
    Query(query): Query<ExecutionsQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let store = match state.backend {
        Some(ref s) => s,
        None => return no_backend().into_response(),
    };
    match store.list_executions(query.plan_id).await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}
