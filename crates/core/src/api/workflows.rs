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
use newton_types::{
    ApiError, BroadcastEvent, NodeState, NodeStatus, WorkflowInstance, WorkflowStatus,
};
use serde::Deserialize;
use std::sync::Arc;
use utoipa::ToSchema;
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
    pub status: Option<WorkflowStatus>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct NodeUpdate {
    status: NodeStatus,
    started_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
    operator_type: Option<String>,
}

/// Flexible update body: supports both legacy WorkflowDefinition format
/// and new status/ended_at update format.
#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct WorkflowUpdateBody {
    workflow_id: Option<String>,
    #[allow(dead_code)]
    definition: Option<serde_json::Value>,
    status: Option<WorkflowStatus>,
    ended_at: Option<DateTime<Utc>>,
}

fn map_store_err(_e: ApiError) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            code: "ERR_INTERNAL".to_string(),
            category: "internal".to_string(),
            message: "Internal storage error".to_string(),
            details: None,
        }),
    )
        .into_response()
}

fn not_found_response(message: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            code: "ERR_NOT_FOUND".to_string(),
            category: "resource".to_string(),
            message: message.to_string(),
            details: None,
        }),
    )
        .into_response()
}

fn conflict_response(message: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(ApiError {
            code: "ERR_CONFLICT".to_string(),
            category: "state".to_string(),
            message: message.to_string(),
            details: None,
        }),
    )
        .into_response()
}

fn validation_response(message: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(ApiError {
            code: "ERR_VALIDATION".to_string(),
            category: "validation".to_string(),
            message: message.to_string(),
            details: None,
        }),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/api/workflows",
    tag = "workflows",
    params(
        ("status" = Option<WorkflowStatus>, Query, description = "Optional workflow status filter"),
        ("limit" = Option<usize>, Query, description = "Maximum number of workflow instances"),
        ("offset" = Option<usize>, Query, description = "Number of workflow instances to skip")
    ),
    responses(
        (status = 200, description = "Workflow instance list", body = [WorkflowInstance])
    )
)]
pub(crate) async fn list_workflows(
    Query(query): Query<WorkflowListQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match state
        .backend
        .list_workflow_instances(query.status, query.limit, query.offset)
        .await
    {
        Ok(instances) => (StatusCode::OK, Json(instances)).into_response(),
        Err(e) => map_store_err(e),
    }
}

#[utoipa::path(
    get,
    path = "/api/workflows/{id}",
    tag = "workflows",
    params(("id" = String, Path, description = "Workflow instance id")),
    responses(
        (status = 200, description = "Workflow instance", body = WorkflowInstance),
        (status = 404, description = "Workflow instance not found", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError)
    )
)]
pub(crate) async fn get_workflow(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    if Uuid::parse_str(&id).is_err() {
        return validation_response("Invalid workflow instance ID format");
    }

    match state.backend.get_workflow_instance(&id).await {
        Ok(instance) => (StatusCode::OK, Json(instance)).into_response(),
        Err(e) if e.code == "ERR_NOT_FOUND" => not_found_response("Workflow instance not found"),
        Err(e) => map_store_err(e),
    }
}

#[utoipa::path(
    post,
    path = "/api/workflows",
    tag = "workflows",
    request_body = WorkflowInstance,
    responses(
        (status = 201, description = "Created workflow instance", body = WorkflowInstance),
        (status = 409, description = "Workflow instance already exists", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError)
    )
)]
pub(crate) async fn create_workflow(
    State(state): State<Arc<AppState>>,
    Json(instance): Json<WorkflowInstance>,
) -> Response {
    if Uuid::parse_str(&instance.instance_id).is_err() {
        return validation_response("Invalid workflow instance ID format");
    }

    // Check for duplicate
    match state
        .backend
        .get_workflow_instance(&instance.instance_id)
        .await
    {
        Ok(_) => return conflict_response("Workflow instance already exists"),
        Err(e) if e.code == "ERR_NOT_FOUND" => {}
        Err(e) => return map_store_err(e),
    }

    // Persist instance row (nodes handled separately below)
    if let Err(e) = state.backend.upsert_workflow_instance(&instance).await {
        return map_store_err(e);
    }

    // Persist any nodes included in the initial payload
    for node in &instance.nodes {
        if let Err(e) = state
            .backend
            .upsert_node_state(&instance.instance_id, node)
            .await
        {
            return map_store_err(e);
        }
    }

    (StatusCode::CREATED, Json(instance)).into_response()
}

#[utoipa::path(
    put,
    path = "/api/workflows/{id}",
    tag = "workflows",
    params(("id" = String, Path, description = "Workflow instance id")),
    request_body = WorkflowUpdateBody,
    responses(
        (status = 200, description = "Updated workflow instance", body = WorkflowInstance),
        (status = 404, description = "Workflow instance not found", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError)
    )
)]
pub(crate) async fn update_workflow(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(body): Json<WorkflowUpdateBody>,
) -> Response {
    if Uuid::parse_str(&id).is_err() {
        return validation_response("Invalid workflow instance ID format");
    }

    let mut instance = match state.backend.get_workflow_instance(&id).await {
        Ok(i) => i,
        Err(e) if e.code == "ERR_NOT_FOUND" => {
            return not_found_response("Workflow instance not found")
        }
        Err(e) => return map_store_err(e),
    };

    if let Some(workflow_id) = body.workflow_id {
        instance.workflow_id = workflow_id;
    }
    if let Some(status) = body.status {
        instance.status = status;
    }
    if let Some(ended_at) = body.ended_at {
        instance.ended_at = Some(ended_at);
    }

    if let Err(e) = state.backend.upsert_workflow_instance(&instance).await {
        return map_store_err(e);
    }

    (StatusCode::OK, Json(instance)).into_response()
}

#[utoipa::path(
    patch,
    path = "/api/workflows/{id}/nodes/{node_id}",
    tag = "workflows",
    params(
        ("id" = String, Path, description = "Workflow instance id"),
        ("node_id" = String, Path, description = "Workflow node id")
    ),
    request_body = NodeUpdate,
    responses(
        (status = 200, description = "Updated workflow instance", body = WorkflowInstance),
        (status = 404, description = "Workflow instance not found", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError)
    )
)]
pub(crate) async fn update_node(
    Path((id, node_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    Json(node_update): Json<NodeUpdate>,
) -> Response {
    if Uuid::parse_str(&id).is_err() {
        return validation_response("Invalid workflow instance ID format");
    }

    if node_id.trim().is_empty() {
        return validation_response("Invalid node ID format");
    }

    // Verify instance exists
    match state.backend.get_workflow_instance(&id).await {
        Ok(_) => {}
        Err(e) if e.code == "ERR_NOT_FOUND" => {
            return not_found_response("Workflow instance not found")
        }
        Err(e) => return map_store_err(e),
    }

    let node = NodeState {
        node_id: node_id.clone(),
        status: node_update.status,
        started_at: node_update.started_at,
        ended_at: node_update.ended_at,
        operator_type: node_update.operator_type,
    };

    if let Err(e) = state.backend.upsert_node_state(&id, &node).await {
        return map_store_err(e);
    }

    let _ = state.events_tx.send(BroadcastEvent::NodeStateChanged {
        instance_id: id.clone(),
        node_id: node_id.clone(),
    });

    // Return full instance with updated nodes
    match state.backend.get_workflow_instance(&id).await {
        Ok(instance) => (StatusCode::OK, Json(instance)).into_response(),
        Err(e) => map_store_err(e),
    }
}
