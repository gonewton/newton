use crate::cli::args::{ImportArgs, ServeArgs};
use crate::cli::workspace_paths::{
    resolve_state_dir, state_backend_sqlite, state_backend_sqlite_url,
};
use crate::cli::WorkspacePaths;
use newton_core::core::error::AppError;
use newton_core::core::types::ErrorCategory;
use newton_core::workflow::schema as workflow_schema;
use std::{fs, path::PathBuf, result::Result as StdResult, sync::Arc};

const NEWTON_REST_ROUTE_PREFIXES: &[&str] = &[
    "/api",
    "/health",
    "/workflows",
    "/hil",
    "/streaming",
    "/operators",
    "/dashboard",
    "/portfolio",
    "/opportunities",
    "/requests",
    "/plans",
    "/persistence",
    "/testing",
];

fn validate_ailoop_path(p: &str) -> StdResult<(), AppError> {
    let invalid =
        p.is_empty() || !p.starts_with('/') || p == "/" || (p.len() > 1 && p.ends_with('/'));
    if invalid {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "NEWTON-SERVE-AIL-001: --ailoop-base-path must start with '/' and must not be '/' or end with '/'; got {:?}",
                p
            ),
        )
        .with_code("NEWTON-SERVE-AIL-001"));
    }
    Ok(())
}

fn ensure_no_ailoop_path_collision(ailoop_path: &str) -> StdResult<(), AppError> {
    for prefix in NEWTON_REST_ROUTE_PREFIXES {
        if ailoop_path == *prefix
            || prefix.starts_with(&format!("{}/", ailoop_path))
            || ailoop_path.starts_with(&format!("{}/", prefix))
        {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                format!(
                    "NEWTON-SERVE-AIL-002: --ailoop-base-path {:?} collides with Newton REST route prefix {:?}",
                    ailoop_path, prefix
                ),
            )
            .with_code("NEWTON-SERVE-AIL-002"));
        }
    }
    Ok(())
}

pub async fn serve(args: ServeArgs) -> StdResult<(), AppError> {
    use cli_framework::api::{
        ApiServerBuilder, ApiVersion, ApiVersionName, DefaultVersion, Stability,
    };
    use newton_core::api::{self, state::AppState};
    use std::net::SocketAddr;
    use tower_http::cors::CorsLayer;
    use tracing::info;

    if args.with_embedded_ailoop {
        validate_ailoop_path(&args.ailoop_base_path)?;
        ensure_no_ailoop_path_collision(&args.ailoop_base_path)?;
    }

    let addr = format!("{}:{}", args.host, args.port);
    let _: SocketAddr = addr.parse().map_err(|err| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("invalid bind address: {err}"),
        )
    })?;

    info!("Starting Newton API server on {}: {}", args.host, args.port);

    let serve_settings: workflow_schema::WorkflowSettings = Default::default();
    let registry = super::build_operator_registry(PathBuf::from("."), &serve_settings, None).await;

    let operator_names = registry.operator_names();
    let operator_descriptors: Vec<newton_types::OperatorDescriptor> = operator_names
        .iter()
        .map(|name: &String| newton_types::OperatorDescriptor {
            operator_type: name.clone(),
            description: format!("{name} operator"),
            params_schema: serde_json::json!({}),
        })
        .collect();

    let workspace_paths = WorkspacePaths::from_cwd().map_err(|e| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to resolve workspace paths: {e}"),
        )
    })?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| workspace_paths.workspace_root.clone());
    let state_dir = resolve_state_dir(&cwd, args.state_dir.as_deref());
    if state_dir.exists() && !state_dir.is_dir() {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "STATE-DIR-001: --state-dir path is not a directory: {}",
                state_dir.display()
            ),
        )
        .with_code("STATE-DIR-001"));
    }
    fs::create_dir_all(&state_dir).map_err(|e| {
        AppError::new(
            ErrorCategory::IoError,
            format!("STATE-DIR-002: failed to create state dir: {e}"),
        )
        .with_code("STATE-DIR-002")
    })?;
    let db_path = state_backend_sqlite(&state_dir);
    let db_url = state_backend_sqlite_url(&state_dir);

    let store = newton_backend::SqliteBackendStore::new(&db_url)
        .await
        .map_err(|e| {
            AppError::new(
                ErrorCategory::IoError,
                format!("STATE-DIR-003: backend store init failed: {}", e.message),
            )
            .with_code("STATE-DIR-003")
        })?;
    info!("Backend store initialized at {}", db_path.display());
    let backend: Arc<dyn newton_backend::BackendStore> = Arc::new(store);

    if args.import_existing {
        let import_args = ImportArgs {
            state_dir: Some(state_dir.clone()),
            workspace: None,
            recursive: false,
        };
        match super::import::workflow_import(import_args).await {
            Ok(()) => {}
            Err(e) => {
                tracing::warn!(error = %e.message, "import_existing scan failed at serve startup")
            }
        }
    }

    let state = AppState::new(operator_descriptors, backend);
    let file_store = newton_core::workflow::file_store::FsWorkflowFileStore::new(
        workspace_paths.workflows_dir.clone(),
    );
    let state = state.with_workflow_files(std::sync::Arc::new(file_store));

    let v1 = api::api_v1_router(state);

    let openapi_value = api::openapi_json();

    let version_name = ApiVersionName::parse("v1").map_err(|e| {
        AppError::new(
            ErrorCategory::InternalError,
            format!("invalid API version name: {e}"),
        )
    })?;

    let mut builder = ApiServerBuilder::new()
        .version(ApiVersion {
            name: version_name.clone(),
            router: v1,
            stability: Stability::Stable,
            deprecation: None,
            openapi: Some(openapi_value),
        })
        .default_version(DefaultVersion::Pinned(version_name))
        .cors(CorsLayer::permissive())
        .health_version(env!("CARGO_PKG_VERSION"));

    // Web UI: the embedded bundle is served at all non-API paths by default;
    // `--no-web` opts out (API only).
    let web_ui_mode: &str = if args.no_web {
        "disabled"
    } else {
        builder = builder.root_fallback(api::embedded_web_router());
        "embedded"
    };
    info!(
        event = "web_ui",
        mode = web_ui_mode,
        "web UI serving mode resolved"
    );

    if args.with_mcp {
        let ctx = crate::cli::context::NewtonContext::new();
        let mcp_router = crate::cli::framework_setup::build_mcp_router_for_serve(ctx, "/mcp")
            .map_err(|err| {
                AppError::new(
                    ErrorCategory::IoError,
                    format!("NEWTON-SERVE-MCP-004: failed to build MCP router: {err}"),
                )
                .with_code("NEWTON-SERVE-MCP-004")
            })?;
        builder = builder.mcp_router(mcp_router);
    }

    let ailoop_state: Option<(
        Arc<ailoop_server::AiloopAppState>,
        ailoop_server::ServeConfig,
    )> = if args.with_embedded_ailoop {
        let ailoop_app_state = Arc::new(ailoop_server::AiloopAppState::new("default"));
        let config = ailoop_server::ServeConfig {
            base_path: None,
            ..Default::default()
        };
        let ailoop_router =
            ailoop_server::router(Arc::clone(&ailoop_app_state), &config).map_err(|e| {
                AppError::new(ErrorCategory::IoError, format!("NEWTON-SERVE-AIL-004: {e}"))
                    .with_code("NEWTON-SERVE-AIL-004")
            })?;
        builder = builder.mount(&args.ailoop_base_path, ailoop_router);
        Some((ailoop_app_state, config))
    } else {
        None
    };

    let server = builder.build();
    let cancel = server.shutdown_token();

    if let Some((ref ailoop_app_state, ref ailoop_config)) = ailoop_state {
        ailoop_server::spawn_background_tasks(
            Arc::clone(ailoop_app_state),
            ailoop_config,
            cancel.clone(),
        );
    }

    if args.with_mcp {
        let bind_address = format!("{}:{}", args.host, args.port);
        let count = crate::cli::mcp::tool_count();
        tracing::info!(
            event = "mcp_serve_started",
            mcp_enabled = true,
            bind_address = %bind_address,
            mcp_path = "/mcp",
            tool_count = count,
            "MCP router mounted on Newton serve listener"
        );
        eprintln!(
            "{{\"event\":\"mcp_serve_started\",\"mcp_enabled\":true,\"bind_address\":\"{}\",\"mcp_path\":\"/mcp\",\"tool_count\":{}}}",
            bind_address, count
        );
    }

    if args.with_embedded_ailoop {
        let bind_address = format!("{}:{}", args.host, args.port);
        tracing::info!(
            event = "ailoop_serve_started",
            ailoop_enabled = true,
            bind_address = %bind_address,
            ailoop_base_path = %args.ailoop_base_path,
            "ailoop embedding active on Newton serve listener"
        );
        eprintln!(
            "{}",
            serde_json::json!({
                "event": "ailoop_serve_started",
                "ailoop_enabled": true,
                "bind_address": bind_address,
                "ailoop_base_path": args.ailoop_base_path,
            })
        );
    }

    server
        .serve(&addr)
        .await
        .map_err(|err| AppError::new(ErrorCategory::IoError, format!("server error: {err}")))?;

    Ok(())
}

#[cfg(test)]
mod serve_ailoop_validation_tests {
    use super::*;

    #[test]
    fn validate_ailoop_path_accepts_normal_paths() {
        assert!(validate_ailoop_path("/ailoop").is_ok());
        assert!(validate_ailoop_path("/hil-server").is_ok());
        assert!(validate_ailoop_path("/embedded/ailoop").is_ok());
    }

    #[test]
    fn validate_ailoop_path_rejects_empty() {
        let err = validate_ailoop_path("").unwrap_err();
        assert!(
            err.message.contains("NEWTON-SERVE-AIL-001"),
            "err={}",
            err.message
        );
    }

    #[test]
    fn validate_ailoop_path_rejects_missing_leading_slash() {
        let err = validate_ailoop_path("ailoop").unwrap_err();
        assert!(
            err.message.contains("NEWTON-SERVE-AIL-001"),
            "err={}",
            err.message
        );
    }

    #[test]
    fn validate_ailoop_path_rejects_bare_root() {
        let err = validate_ailoop_path("/").unwrap_err();
        assert!(
            err.message.contains("NEWTON-SERVE-AIL-001"),
            "err={}",
            err.message
        );
    }

    #[test]
    fn validate_ailoop_path_rejects_trailing_slash() {
        let err = validate_ailoop_path("/ailoop/").unwrap_err();
        assert!(
            err.message.contains("NEWTON-SERVE-AIL-001"),
            "err={}",
            err.message
        );
    }

    #[test]
    fn validate_ailoop_path_accepts_api() {
        assert!(validate_ailoop_path("/api").is_ok());
    }

    #[test]
    fn ailoop_collision_detects_health() {
        let err = ensure_no_ailoop_path_collision("/health").unwrap_err();
        assert!(
            err.message.contains("NEWTON-SERVE-AIL-002"),
            "err={}",
            err.message
        );
    }

    #[test]
    fn ailoop_collision_detects_api() {
        let err = ensure_no_ailoop_path_collision("/api").unwrap_err();
        assert!(
            err.message.contains("NEWTON-SERVE-AIL-002"),
            "err={}",
            err.message
        );
    }

    #[test]
    fn ailoop_collision_detects_workflows() {
        let err = ensure_no_ailoop_path_collision("/workflows").unwrap_err();
        assert!(
            err.message.contains("NEWTON-SERVE-AIL-002"),
            "err={}",
            err.message
        );
    }

    #[test]
    fn ailoop_collision_detects_ancestor_of_prefix() {
        let err = ensure_no_ailoop_path_collision("/health/sub").unwrap_err();
        assert!(
            err.message.contains("NEWTON-SERVE-AIL-002"),
            "err={}",
            err.message
        );
    }

    #[test]
    fn ailoop_collision_allows_unrelated_path() {
        assert!(ensure_no_ailoop_path_collision("/ailoop").is_ok());
    }

    #[test]
    fn ailoop_collision_checks_all_newton_prefixes() {
        for prefix in NEWTON_REST_ROUTE_PREFIXES {
            assert!(
                ensure_no_ailoop_path_collision(prefix).is_err(),
                "expected collision for prefix {prefix}"
            );
        }
    }
}
