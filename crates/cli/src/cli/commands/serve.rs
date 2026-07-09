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

/// True when `host` resolves to a loopback interface (127.0.0.0/8, `::1`) or
/// the `localhost` hostname. `--host` defaults to `127.0.0.1`; passing
/// anything else is the operator's explicit opt-in to wider exposure (see
/// spec 074 PR-6 / B5 — no separate `--allow-remote`-style flag is added).
fn is_loopback_host(host: &str) -> bool {
    let trimmed = host.trim_start_matches('[').trim_end_matches(']');
    if trimmed.eq_ignore_ascii_case("localhost") {
        return true;
    }
    trimmed
        .parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Checks whether `host` is a non-loopback bind target and, if so, emits the
/// `unauthenticated_exposure` warning event. Returns `true` when the bind is
/// non-loopback (the caller uses this to also print the louder startup-banner
/// warning). Extracted from `serve()`'s body so the check/warn decision is
/// unit-testable without starting a real HTTP listener (spec 074 PR-6 / B5).
fn check_non_loopback_bind(host: &str, port: u16) -> bool {
    let non_loopback_bind = !is_loopback_host(host);
    if non_loopback_bind {
        tracing::warn!(
            event = "unauthenticated_exposure",
            host = %host,
            port = port,
            "newton serve is binding a non-loopback host; the Newton HTTP API is UNAUTHENTICATED and will be reachable from other hosts on this interface"
        );
    }
    non_loopback_bind
}

/// Builds the human-readable startup banner lines. Newton's `info!` startup
/// logs are silenced in the serve (Server) console context, and
/// cli-framework's `serve()` prints nothing, so without this `newton serve`
/// looks like it hangs with no output. Pure so the banner text — including
/// the non-loopback exposure warning block — can be unit tested without
/// starting a real HTTP listener.
#[allow(clippy::too_many_arguments)]
fn startup_banner_lines(
    host: &str,
    port: u16,
    web_ui_mode: &str,
    with_mcp: bool,
    with_embedded_ailoop: bool,
    ailoop_base_path: &str,
    non_loopback_bind: bool,
) -> Vec<String> {
    // 0.0.0.0 / :: aren't browsable; point the user at a loopback address.
    let browse_host = match host {
        "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
        h => h,
    };
    let base = format!("http://{browse_host}:{port}");
    let mut lines = Vec::new();
    lines.push(String::new());
    lines.push(format!("  Newton serving on {base}"));
    if web_ui_mode != "disabled" {
        lines.push(format!("    Web UI     {base}/"));
    }
    lines.push(format!("    REST API   {base}/api/v1/"));
    lines.push(format!("    Health     {base}/healthz"));
    lines.push(format!("    API docs   {base}/api/docs"));
    if with_mcp {
        lines.push(format!("    MCP        {base}/mcp"));
    }
    if with_embedded_ailoop {
        lines.push(format!("    ailoop     {base}{ailoop_base_path}"));
    }
    if web_ui_mode == "disabled" {
        lines.push("    (web UI disabled via --no-web)".to_string());
    }
    if non_loopback_bind {
        lines.push(String::new());
        lines.push(
            "  !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!".to_string(),
        );
        lines.push(format!(
            "  !! WARNING: bound to non-loopback host \"{}\" — the Newton API is    !!",
            host
        ));
        lines.push(
            "  !! UNAUTHENTICATED and exposed to any host that can reach this    !!".to_string(),
        );
        lines.push(
            "  !! interface. --host is your explicit opt-in; real authentication  !!".to_string(),
        );
        lines.push(
            "  !! is not yet implemented (deferred to a future spec).             !!".to_string(),
        );
        lines.push(
            "  !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!".to_string(),
        );
    }
    lines.push("  Press Ctrl+C to stop.".to_string());
    lines.push(String::new());
    lines
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

    let non_loopback_bind = check_non_loopback_bind(&args.host, args.port);

    let workspace_paths = WorkspacePaths::from_cwd().map_err(|e| {
        AppError::new(
            ErrorCategory::IoError,
            format!("failed to resolve workspace paths: {e}"),
        )
    })?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| workspace_paths.workspace_root.clone());
    // Resolve once, up front, so both the operator registry's grading-operator
    // store and the AppState backend below open the SAME database — the split
    // brain this hardening pass closes.
    let state_dir = resolve_state_dir(&cwd, args.state_dir.as_deref());

    let serve_settings: workflow_schema::WorkflowSettings = Default::default();
    let registry =
        super::build_operator_registry(PathBuf::from("."), &state_dir, &serve_settings, None).await;

    let operator_names = registry.operator_names();
    let operator_descriptors: Vec<newton_types::OperatorDescriptor> = operator_names
        .iter()
        .map(|name: &String| newton_types::OperatorDescriptor {
            operator_type: name.clone(),
            description: format!("{name} operator"),
            params_schema: serde_json::json!({}),
        })
        .collect();

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

    // Human-readable startup banner (see `startup_banner_lines` doc comment
    // for why this exists).
    for line in startup_banner_lines(
        &args.host,
        args.port,
        web_ui_mode,
        args.with_mcp,
        args.with_embedded_ailoop,
        &args.ailoop_base_path,
        non_loopback_bind,
    ) {
        eprintln!("{line}");
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

#[cfg(test)]
mod is_loopback_host_tests {
    use super::*;

    #[test]
    fn ipv4_loopback_is_loopback() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.0.0.5"));
    }

    #[test]
    fn ipv4_non_loopback_is_not_loopback() {
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("203.0.113.5"));
        assert!(!is_loopback_host("10.0.0.1"));
    }

    #[test]
    fn ipv6_loopback_is_loopback() {
        assert!(is_loopback_host("::1"));
    }

    #[test]
    fn bracketed_ipv6_loopback_is_loopback() {
        assert!(is_loopback_host("[::1]"));
    }

    #[test]
    fn ipv6_non_loopback_is_not_loopback() {
        assert!(!is_loopback_host("::"));
        assert!(!is_loopback_host("2001:db8::1"));
        assert!(!is_loopback_host("[2001:db8::1]"));
    }

    #[test]
    fn localhost_hostname_is_loopback_case_insensitive() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("LOCALHOST"));
        assert!(is_loopback_host("LocalHost"));
    }

    #[test]
    fn arbitrary_hostname_is_not_loopback() {
        assert!(!is_loopback_host("example.com"));
        assert!(!is_loopback_host("my-host"));
    }

    #[test]
    fn empty_host_is_not_loopback() {
        assert!(!is_loopback_host(""));
    }
}

#[cfg(test)]
mod non_loopback_warn_tests {
    use super::*;

    #[test]
    fn loopback_host_does_not_warn() {
        assert!(!check_non_loopback_bind("127.0.0.1", 3000));
        assert!(!check_non_loopback_bind("localhost", 3000));
    }

    #[test]
    fn non_loopback_host_warns_and_returns_true() {
        assert!(check_non_loopback_bind("0.0.0.0", 3000));
        assert!(check_non_loopback_bind("203.0.113.5", 8080));
    }
}

#[cfg(test)]
mod startup_banner_tests {
    use super::*;

    #[test]
    fn loopback_bind_has_no_warning_banner() {
        let lines = startup_banner_lines(
            "127.0.0.1",
            3000,
            "embedded",
            false,
            false,
            "/ailoop",
            false,
        );
        let joined = lines.join("\n");
        assert!(joined.contains("Newton serving on http://127.0.0.1:3000"));
        assert!(joined.contains("Web UI     http://127.0.0.1:3000/"));
        assert!(!joined.contains("WARNING"));
    }

    #[test]
    fn non_loopback_bind_includes_warning_banner() {
        let lines =
            startup_banner_lines("0.0.0.0", 3000, "embedded", false, false, "/ailoop", true);
        let joined = lines.join("\n");
        assert!(
            joined.contains("WARNING: bound to non-loopback host \"0.0.0.0\""),
            "{joined}"
        );
        assert!(joined.contains("UNAUTHENTICATED and exposed"));
    }

    #[test]
    fn unspecified_bind_addresses_map_to_browsable_loopback() {
        let lines =
            startup_banner_lines("0.0.0.0", 3000, "embedded", false, false, "/ailoop", false);
        assert!(lines.iter().any(|l| l.contains("http://127.0.0.1:3000")));

        let lines = startup_banner_lines("::", 3000, "embedded", false, false, "/ailoop", false);
        assert!(lines.iter().any(|l| l.contains("http://127.0.0.1:3000")));

        let lines = startup_banner_lines("[::]", 3000, "embedded", false, false, "/ailoop", false);
        assert!(lines.iter().any(|l| l.contains("http://127.0.0.1:3000")));
    }

    #[test]
    fn disabled_web_ui_mode_omits_web_ui_line_and_notes_disabled() {
        let lines = startup_banner_lines(
            "127.0.0.1",
            3000,
            "disabled",
            false,
            false,
            "/ailoop",
            false,
        );
        let joined = lines.join("\n");
        assert!(!joined.contains("Web UI"));
        assert!(joined.contains("(web UI disabled via --no-web)"));
    }

    #[test]
    fn mcp_enabled_adds_mcp_line() {
        let lines =
            startup_banner_lines("127.0.0.1", 3000, "embedded", true, false, "/ailoop", false);
        assert!(lines
            .iter()
            .any(|l| l.contains("MCP        http://127.0.0.1:3000/mcp")));
    }

    #[test]
    fn ailoop_enabled_adds_ailoop_line_with_base_path() {
        let lines = startup_banner_lines("127.0.0.1", 3000, "embedded", false, true, "/hil", false);
        assert!(lines
            .iter()
            .any(|l| l.contains("ailoop     http://127.0.0.1:3000/hil")));
    }

    #[test]
    fn always_ends_with_press_ctrl_c() {
        let lines = startup_banner_lines(
            "127.0.0.1",
            3000,
            "embedded",
            false,
            false,
            "/ailoop",
            false,
        );
        assert!(lines.iter().any(|l| l.contains("Press Ctrl+C to stop.")));
    }
}
