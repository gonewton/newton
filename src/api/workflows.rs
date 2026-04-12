use crate::api::state::AppState;
use axum::{
    extract::Path,
    extract::Query,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, patch, post, put},
    Router,
};
use chrono::{DateTime, Utc};
use newton_types::{ApiError, BroadcastEvent, NodeStatus, WorkflowInstance, WorkflowStatus};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

/// Routes for the workflows API resource.
pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/workflows", get(list_workflows))
        .route("/api/workflows", post(create_workflow))
        .route("/api/workflows/{id}", get(get_workflow))
        .route("/api/workflows/{id}", put(update_workflow))
        .route("/api/workflows/{id}/nodes/{node_id}", patch(update_node))
        .with_state(state)
}

/// Query parameters for listing workflow instances.
#[derive(Debug, Deserialize)]
pub struct WorkflowListQuery {
    /// Optional status filter.
    pub status: Option<WorkflowStatus>,
    /// Maximum number of items to return.
    pub limit: Option<usize>,
    /// Number of items to skip before collecting results.
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct NodeUpdate {
    status: NodeStatus,
    started_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
    operator_type: Option<String>,
}

/// Flexible update body: supports both legacy WorkflowDefinition format
/// and new status/ended_at update format.
#[derive(Debug, Deserialize)]
struct WorkflowUpdateBody {
    workflow_id: Option<String>,
    #[allow(dead_code)]
    definition: Option<serde_json::Value>,
    status: Option<WorkflowStatus>,
    ended_at: Option<DateTime<Utc>>,
}

async fn list_workflows(
    Query(query): Query<WorkflowListQuery>,
    State(state): State<Arc<AppState>>,
) -> Json<Vec<WorkflowInstance>> {
    let mut instances: Vec<WorkflowInstance> = state
        .instances
        .iter()
        .map(|entry| entry.value().clone())
        .collect();

    if let Some(ref status) = query.status {
        instances.retain(|i| &i.status == status);
    }

    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(usize::MAX);
    instances = instances.into_iter().skip(offset).take(limit).collect();

    Json(instances)
}

async fn get_workflow(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> Response {
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

async fn create_workflow(
    State(state): State<Arc<AppState>>,
    Json(instance): Json<WorkflowInstance>,
) -> Response {
    if Uuid::parse_str(&instance.instance_id).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "API-WORKFLOW-001".to_string(),
                category: "ValidationError".to_string(),
                message: "Invalid workflow instance ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    if state.instances.contains_key(&instance.instance_id) {
        return (
            StatusCode::CONFLICT,
            Json(ApiError {
                code: "API-WORKFLOW-003".to_string(),
                category: "ValidationError".to_string(),
                message: "Workflow instance already exists".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    state
        .instances
        .insert(instance.instance_id.clone(), instance.clone());
    (StatusCode::CREATED, Json(instance)).into_response()
}

async fn update_workflow(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<WorkflowUpdateBody>,
) -> Response {
    if Uuid::parse_str(&id).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "API-WORKFLOW-001".to_string(),
                category: "ValidationError".to_string(),
                message: "Invalid workflow instance ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    if let Some(mut instance) = state.instances.get_mut(&id) {
        if let Some(workflow_id) = body.workflow_id {
            instance.workflow_id = workflow_id;
        }
        if let Some(status) = body.status {
            instance.status = status;
        }
        if let Some(ended_at) = body.ended_at {
            instance.ended_at = Some(ended_at);
        }
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

async fn update_node(
    Path((id, node_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(node_update): Json<NodeUpdate>,
) -> Response {
    if Uuid::parse_str(&id).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "API-WORKFLOW-001".to_string(),
                category: "ValidationError".to_string(),
                message: "Invalid workflow instance ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    if node_id.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "API-NODE-001".to_string(),
                category: "ValidationError".to_string(),
                message: "Invalid node ID format".to_string(),
                details: None,
            }),
        )
            .into_response();
    }

    match state.instances.get_mut(&id) {
        Some(mut instance) => {
            if let Some(node) = instance.nodes.iter_mut().find(|n| n.node_id == node_id) {
                node.status = node_update.status;
                if node_update.started_at.is_some() {
                    node.started_at = node_update.started_at;
                }
                node.ended_at = node_update.ended_at;
                if node_update.operator_type.is_some() {
                    node.operator_type = node_update.operator_type;
                }
            } else {
                let new_node = newton_types::NodeState {
                    node_id: node_id.clone(),
                    status: node_update.status,
                    started_at: node_update.started_at,
                    ended_at: node_update.ended_at,
                    operator_type: node_update.operator_type,
                };
                instance.nodes.push(new_node);
            }

            let _ = state.events_tx.send(BroadcastEvent::NodeStateChanged {
                instance_id: id.clone(),
                node_id: node_id.clone(),
            });

            (StatusCode::OK, Json(instance.clone())).into_response()
        }
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
