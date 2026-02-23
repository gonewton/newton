#![allow(clippy::result_large_err)] // Webhook helpers return AppError for consistent diagnostics.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::{
    executor::{spawn_workflow_execution, ExecutionOverrides},
    operator::OperatorRegistry,
    schema::{self, TriggerType, WorkflowDocument, WorkflowTrigger},
};
use axum::{
    body::{Body, Bytes},
    extract::Extension,
    http::{header, HeaderMap, HeaderValue, Response, StatusCode},
    response::{IntoResponse, Json},
    routing::post,
    Router,
};
use serde_json::{json, Value};
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tower::util::MapResponseLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tracing::info;

/// State shared across webhook requests.
struct WebhookState {
    workflow_path: PathBuf,
    workspace_root: PathBuf,
    document: Arc<WorkflowDocument>,
    registry: OperatorRegistry,
    overrides: ExecutionOverrides,
    auth_token: String,
}

/// Start the webhook listener and block until the service terminates.
pub async fn serve_webhook(
    document: WorkflowDocument,
    workflow_path: PathBuf,
    registry: OperatorRegistry,
    workspace_root: PathBuf,
    overrides: ExecutionOverrides,
) -> Result<(), AppError> {
    serve_webhook_internal(
        document,
        workflow_path,
        registry,
        workspace_root,
        overrides,
        None,
    )
    .await
}

/// Start the webhook listener and notify once the bind address is known (test helper).
pub async fn serve_webhook_with_ready_notifier(
    document: WorkflowDocument,
    workflow_path: PathBuf,
    registry: OperatorRegistry,
    workspace_root: PathBuf,
    overrides: ExecutionOverrides,
    ready_notifier: oneshot::Sender<SocketAddr>,
) -> Result<(), AppError> {
    serve_webhook_internal(
        document,
        workflow_path,
        registry,
        workspace_root,
        overrides,
        Some(ready_notifier),
    )
    .await
}

async fn serve_webhook_internal(
    document: WorkflowDocument,
    workflow_path: PathBuf,
    registry: OperatorRegistry,
    workspace_root: PathBuf,
    overrides: ExecutionOverrides,
    ready_notifier: Option<oneshot::Sender<SocketAddr>>,
) -> Result<(), AppError> {
    let settings = document.workflow.settings.webhook.clone();
    if !settings.enabled {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "webhook must be enabled under workflow settings to start listener",
        )
        .with_code("WFG-WEBHOOK-000"));
    }
    let auth_token = load_auth_token(&settings)?;
    let bind_addr: SocketAddr = settings.bind.parse().map_err(|err| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("invalid webhook bind address {}: {}", settings.bind, err),
        )
    })?;
    let state = Arc::new(WebhookState {
        workflow_path: workflow_path.clone(),
        workspace_root: workspace_root.clone(),
        document: Arc::new(document),
        registry,
        overrides,
        auth_token,
    });
    let router = Router::new()
        .route("/v1/workflow/trigger", post(handle_trigger))
        .layer(Extension(state))
        .layer(RequestBodyLimitLayer::new(settings.max_body_bytes))
        .layer(MapResponseLayer::new(|mut response: Response<Body>| {
            if response.status() == StatusCode::PAYLOAD_TOO_LARGE {
                let body = json!({
                    "error": {
                        "code": "WFG-WEBHOOK-413",
                        "message": "payload too large"
                    }
                })
                .to_string();
                *response.body_mut() = Body::from(body);
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
            }
            response
        }));
    let listener = TcpListener::bind(bind_addr).await.map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to bind webhook listener {}: {}", bind_addr, err),
        )
    })?;
    let local_addr = listener.local_addr().map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to determine webhook listener address: {}", err),
        )
    })?;
    if let Some(tx) = ready_notifier {
        let _ = tx.send(local_addr);
    }
    info!("webhook server listening on {}", local_addr);
    axum::serve(listener, router.into_make_service())
        .await
        .map_err(|err| {
            AppError::new(
                ErrorCategory::ResourceError,
                format!("webhook server terminated: {}", err),
            )
        })
}

async fn handle_trigger(
    Extension(state): Extension<Arc<WebhookState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, WebhookRejection> {
    if !is_authorized(&headers, &state.auth_token) {
        return Err(WebhookRejection::unauthorized());
    }
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|_| WebhookRejection::bad_request("invalid JSON payload"))?;
    let trigger_value = payload
        .get("trigger")
        .cloned()
        .ok_or_else(|| WebhookRejection::bad_request("missing trigger field"))?;
    let trigger: WorkflowTrigger = serde_json::from_value(trigger_value)
        .map_err(|_| WebhookRejection::bad_request("invalid trigger object"))?;
    if trigger.trigger_type != TriggerType::Webhook {
        return Err(WebhookRejection::bad_request(
            "trigger type must be webhook",
        ));
    }
    if !trigger.payload.is_object() {
        return Err(WebhookRejection::bad_request(
            "trigger payload must be an object",
        ));
    }
    let mut document = state.document.as_ref().clone();
    document.triggers = Some(trigger);
    let overrides = state.overrides.clone();
    let registry = state.registry.clone();
    let workspace = state.workspace_root.clone();
    let workflow_path = state.workflow_path.clone();
    let (execution_id, _handle) =
        spawn_workflow_execution(document, workflow_path, registry, workspace, overrides)
            .map_err(WebhookRejection::internal)?;
    Ok(Json(json!({
        "execution_id": execution_id.to_string(),
        "status": "running",
    })))
}

fn load_auth_token(settings: &schema::WebhookSettings) -> Result<String, AppError> {
    let token = env::var(&settings.auth_token_env).map_err(|_| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "webhook auth token environment variable {} is not set",
                settings.auth_token_env
            ),
        )
    })?;
    if token.trim().is_empty() {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "webhook auth token environment variable {} is empty",
                settings.auth_token_env
            ),
        ));
    }
    Ok(token)
}

fn is_authorized(headers: &HeaderMap, expected: &str) -> bool {
    let header_value = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim);
    if let Some(token) = header_value {
        token.as_bytes().ct_eq(expected.as_bytes()).into()
    } else {
        false
    }
}

struct WebhookRejection {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl WebhookRejection {
    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "WFG-WEBHOOK-401",
            message: "unauthorized",
        }
    }

    fn bad_request(message: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "WFG-WEBHOOK-400",
            message,
        }
    }

    fn internal(err: AppError) -> Self {
        tracing::error!("webhook execution error: {}", err);
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "WFG-WEBHOOK-500",
            message: "internal server error",
        }
    }
}

impl IntoResponse for WebhookRejection {
    fn into_response(self) -> Response<Body> {
        let mut resp = Json(json!({
            "error": {
                "code": self.code,
                "message": self.message
            }
        }))
        .into_response();
        *resp.status_mut() = self.status;
        resp
    }
}
