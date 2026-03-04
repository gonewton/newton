use crate::api::state::AppState;
use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use newton_types::{ApiError, WorkflowDefinition, WorkflowInstance};
use std::sync::Arc;
use uuid::Uuid;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/workflows", get(list_workflows))
        .route("/api/workflows/{id}", get(get_workflow))
        .route("/api/workflows/{id}", axum::routing::put(update_workflow))
        .with_state(state)
}

async fn list_workflows(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Json<Vec<WorkflowInstance>> {
    let instances: Vec<WorkflowInstance> = state
        .instances
        .iter()
        .map(|entry| entry.value().clone())
        .collect();
    Json(instances)
}

async fn get_workflow(
    Path(id): Path<String>,
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Response {
    match Uuid::parse_str(&id) {
        Ok(_) => {}
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "API-WORKFLOW-001".to_string(),
                    category: "ValidationError".to_string(),
                    message: "Invalid workflow instance ID format".to_string(),
                    details: None,
                }),
            )
                .into_response()
        }
    }

    match state.instances.get(&id) {
        Some(instance) => (StatusCode::OK, Json(instance.clone())).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "API-WORKFLOW-002".to_string(),
                category: "ValidationError".to_string(),
                message: "Workflow instance not found".to_string(),
                details: None,
            }),
        )
            .into_response(),
    }
}

async fn update_workflow(
    Path(id): Path<String>,
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(definition): Json<WorkflowDefinition>,
) -> Response {
    match Uuid::parse_str(&id) {
        Ok(_) => {}
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "API-WORKFLOW-001".to_string(),
                    category: "ValidationError".to_string(),
                    message: "Invalid workflow instance ID format".to_string(),
                    details: None,
                }),
            )
                .into_response()
        }
    }

    if let Some(mut instance) = state.instances.get_mut(&id) {
        instance.workflow_id = definition.workflow_id;
        (StatusCode::OK, Json(instance.clone())).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "API-WORKFLOW-002".to_string(),
                category: "ValidationError".to_string(),
                message: "Workflow instance not found".to_string(),
                details: None,
            }),
        )
            .into_response()
    }
}
