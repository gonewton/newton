use crate::api::state::AppState;
use crate::workflow::expression::ExpressionEngine;
use crate::workflow::file_store::WriteOutcome;
use crate::workflow::lint::LintRegistry;
use crate::workflow::lint::LintResult;
use crate::workflow::schema::WorkflowDocument;
use crate::workflow::transform;
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::{DateTime, Utc};
use newton_types::ApiError;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

fn err_503() -> Response {
    let e = ApiError {
        code: "ERR_INTERNAL".to_string(),
        category: "InternalError".to_string(),
        message: "workflow file store not configured".to_string(),
        details: None,
    };
    (StatusCode::SERVICE_UNAVAILABLE, Json(e)).into_response()
}

fn err_validation(msg: impl Into<String>) -> Response {
    let e = ApiError {
        code: "ERR_VALIDATION".to_string(),
        category: "ValidationError".to_string(),
        message: msg.into(),
        details: None,
    };
    (StatusCode::UNPROCESSABLE_ENTITY, Json(e)).into_response()
}

fn err_not_found(msg: impl Into<String>) -> Response {
    let e = ApiError {
        code: "ERR_NOT_FOUND".to_string(),
        category: "NotFound".to_string(),
        message: msg.into(),
        details: None,
    };
    (StatusCode::NOT_FOUND, Json(e)).into_response()
}

fn err_conflict(msg: impl Into<String>) -> Response {
    let e = ApiError {
        code: "ERR_CONFLICT".to_string(),
        category: "Conflict".to_string(),
        message: msg.into(),
        details: None,
    };
    (StatusCode::CONFLICT, Json(e)).into_response()
}

fn err_500(msg: impl Into<String>) -> Response {
    let e = ApiError {
        code: "ERR_INTERNAL".to_string(),
        category: "InternalError".to_string(),
        message: msg.into(),
        details: None,
    };
    (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response()
}

fn map_store_error(e: &crate::core::error::AppError) -> Response {
    match e.code.as_str() {
        "ERR_NOT_FOUND" => err_not_found(&e.message),
        "ERR_CONFLICT" => err_conflict(&e.message),
        "ERR_VALIDATION" => err_validation(&e.message),
        _ => err_500(&e.message),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowFileSummary {
    pub name: String,
    pub metadata_name: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub size_bytes: u64,
    pub modified_at: DateTime<Utc>,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowFileDetail {
    pub name: String,
    pub content: String,
    pub content_hash: String,
    pub modified_at: DateTime<Utc>,
    pub diagnostics: WorkflowFileDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PutWorkflowFileBody {
    pub content: String,
    pub expected_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WorkflowFileDiagnostics {
    pub parse_ok: bool,
    pub validate_ok: bool,
    pub parse_error: Option<String>,
    pub lint: Vec<LintFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LintFinding {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub location: Option<String>,
    pub suggestion: Option<String>,
}

impl From<LintResult> for LintFinding {
    fn from(r: LintResult) -> Self {
        LintFinding {
            code: r.code,
            severity: r.severity.to_string(),
            message: r.message,
            location: r.location,
            suggestion: r.suggestion,
        }
    }
}

fn run_diagnostics(content: &str) -> WorkflowFileDiagnostics {
    // Step 1: parse
    let parsed: WorkflowDocument = match serde_yaml::from_str(content) {
        Ok(doc) => doc,
        Err(e) => {
            return WorkflowFileDiagnostics {
                parse_ok: false,
                validate_ok: false,
                parse_error: Some(e.to_string()),
                lint: vec![],
            };
        }
    };
    // Step 2: transform
    let transformed = match transform::apply_default_pipeline(parsed) {
        Ok(doc) => doc,
        Err(e) => {
            return WorkflowFileDiagnostics {
                parse_ok: true,
                validate_ok: false,
                parse_error: None,
                lint: vec![LintFinding {
                    code: "WFG-TRANSFORM".to_string(),
                    severity: "error".to_string(),
                    message: e.message.clone(),
                    location: None,
                    suggestion: None,
                }],
            };
        }
    };
    // Step 3: validate
    let engine = ExpressionEngine::default();
    let validate_ok = transformed.validate(&engine).is_ok();
    // Step 4: lint
    let lint_results = LintRegistry::new().run(&transformed);
    let lint: Vec<LintFinding> = lint_results.into_iter().map(LintFinding::from).collect();
    WorkflowFileDiagnostics {
        parse_ok: true,
        validate_ok,
        parse_error: None,
        lint,
    }
}

fn extract_metadata(content: &str) -> (Option<String>, Option<String>, Option<Vec<String>>) {
    match serde_yaml::from_str::<WorkflowDocument>(content) {
        Ok(doc) => {
            let metadata_name = doc.metadata.as_ref().and_then(|m| m.name.clone());
            let description = doc.metadata.as_ref().and_then(|m| m.description.clone());
            let tags = doc.metadata.as_ref().and_then(|m| m.tags.clone());
            (metadata_name, description, tags)
        }
        Err(_) => (None, None, None),
    }
}

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/workflow-files", get(list_workflow_files))
        .route("/api/workflow-files/validate", post(validate_workflow_file))
        .route("/api/workflow-files/{name}", get(get_workflow_file))
        .route("/api/workflow-files/{name}", put(put_workflow_file))
        .route("/api/workflow-files/{name}", delete(delete_workflow_file))
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/api/workflow-files",
    tag = "workflow-files",
    responses(
        (status = 200, description = "List of workflow files", body = Vec<WorkflowFileSummary>),
        (status = 503, description = "File store not configured", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub(crate) async fn list_workflow_files(State(state): State<Arc<AppState>>) -> Response {
    let store = match &state.workflow_files {
        Some(s) => s.clone(),
        None => return err_503(),
    };
    match store.list() {
        Ok(records) => {
            let summaries: Vec<WorkflowFileSummary> = records
                .iter()
                .map(|r| {
                    let (metadata_name, description, tags) = extract_metadata(&r.content);
                    WorkflowFileSummary {
                        name: r.name.clone(),
                        metadata_name,
                        description,
                        tags,
                        size_bytes: r.size_bytes,
                        modified_at: r.modified_at,
                        content_hash: r.content_hash.clone(),
                    }
                })
                .collect();
            (StatusCode::OK, Json(summaries)).into_response()
        }
        Err(e) => map_store_error(&e),
    }
}

#[utoipa::path(
    get,
    path = "/api/workflow-files/{name}",
    tag = "workflow-files",
    params(("name" = String, Path, description = "Workflow file name (no .yaml extension)")),
    responses(
        (status = 200, description = "Workflow file detail", body = WorkflowFileDetail),
        (status = 404, description = "Not found", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 503, description = "File store not configured", body = ApiError)
    )
)]
pub(crate) async fn get_workflow_file(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    let store = match &state.workflow_files {
        Some(s) => s.clone(),
        None => return err_503(),
    };
    match store.read(&name) {
        Ok(record) => {
            let diagnostics = run_diagnostics(&record.content);
            let etag = format!("\"{}\"", record.content_hash);
            let detail = WorkflowFileDetail {
                name: record.name,
                content: record.content,
                content_hash: record.content_hash,
                modified_at: record.modified_at,
                diagnostics,
            };
            (StatusCode::OK, [(header::ETAG, etag)], Json(detail)).into_response()
        }
        Err(e) => map_store_error(&e),
    }
}

#[utoipa::path(
    put,
    path = "/api/workflow-files/{name}",
    tag = "workflow-files",
    params(("name" = String, Path, description = "Workflow file name (no .yaml extension)")),
    request_body = PutWorkflowFileBody,
    responses(
        (status = 200, description = "Updated workflow file", body = WorkflowFileDetail),
        (status = 201, description = "Created workflow file", body = WorkflowFileDetail),
        (status = 409, description = "ETag conflict", body = ApiError),
        (status = 422, description = "Validation error or unparseable YAML", body = ApiError),
        (status = 503, description = "File store not configured", body = ApiError)
    )
)]
pub(crate) async fn put_workflow_file(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    headers: axum::http::HeaderMap,
    Json(body): Json<PutWorkflowFileBody>,
) -> Response {
    let store = match &state.workflow_files {
        Some(s) => s.clone(),
        None => return err_503(),
    };

    // Parse YAML first (422 if fails)
    if serde_yaml::from_str::<WorkflowDocument>(&body.content).is_err() {
        return err_validation("content is not parseable as a WorkflowDocument");
    }

    // Extract If-Match from header or body
    let if_match: Option<String> = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim_matches('"').to_string())
        .or_else(|| body.expected_hash.clone());

    match store.write(&name, &body.content, if_match.as_deref()) {
        Ok(outcome) => {
            let diagnostics = run_diagnostics(&body.content);
            let bytes = body.content.as_bytes();
            let content_hash = crate::workflow::state::compute_sha256_hex(bytes);
            let status = match outcome {
                WriteOutcome::Created => StatusCode::CREATED,
                WriteOutcome::Updated => StatusCode::OK,
            };
            let detail = WorkflowFileDetail {
                name: name.clone(),
                content: body.content,
                content_hash,
                modified_at: Utc::now(),
                diagnostics,
            };
            (status, Json(detail)).into_response()
        }
        Err(e) => map_store_error(&e),
    }
}

#[utoipa::path(
    delete,
    path = "/api/workflow-files/{name}",
    tag = "workflow-files",
    params(("name" = String, Path, description = "Workflow file name (no .yaml extension)")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 404, description = "Not found", body = ApiError),
        (status = 422, description = "Validation error", body = ApiError),
        (status = 503, description = "File store not configured", body = ApiError)
    )
)]
pub(crate) async fn delete_workflow_file(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Response {
    let store = match &state.workflow_files {
        Some(s) => s.clone(),
        None => return err_503(),
    };
    match store.delete(&name) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => map_store_error(&e),
    }
}

#[utoipa::path(
    post,
    path = "/api/workflow-files/validate",
    tag = "workflow-files",
    request_body = PutWorkflowFileBody,
    responses(
        (status = 200, description = "Diagnostics", body = WorkflowFileDiagnostics),
        (status = 422, description = "Unparseable YAML", body = ApiError)
    )
)]
pub(crate) async fn validate_workflow_file(
    State(_state): State<Arc<AppState>>,
    Json(body): Json<PutWorkflowFileBody>,
) -> Response {
    // Validate is stateless — no file store required
    if serde_yaml::from_str::<WorkflowDocument>(&body.content).is_err() {
        return err_validation("content is not parseable as a WorkflowDocument");
    }
    let diagnostics = run_diagnostics(&body.content);
    (StatusCode::OK, Json(diagnostics)).into_response()
}
