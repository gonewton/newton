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
pub(crate) fn is_loopback_host(host: &str) -> bool {
    let trimmed = host.trim_start_matches('[').trim_end_matches(']');
    if trimmed.eq_ignore_ascii_case("localhost") {
        return true;
    }
    trimmed
        .parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Effective OIDC configuration for `newton serve`, resolved from
/// `--oidc-issuer` / `--oidc-audience` (env-var fallback: `NEWTON_OIDC_ISSUER`
/// / `NEWTON_OIDC_AUDIENCE`, comma-separated). A bare struct rather than the
/// framework's `OidcValidationConfig` so `resolve_oidc_config_from` stays a
/// pure, dependency-free function that's trivial to unit test (audit finding
/// C5).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedOidcConfig {
    issuer: String,
    audiences: Vec<String>,
    /// Public OAuth client id for the SPA's PKCE flow (`--oidc-client-id` /
    /// `NEWTON_OIDC_CLIENT_ID`). Unlike `issuer`/`audiences`, this is always
    /// optional -- the backend never needs it to validate tokens, so its
    /// absence never turns OIDC on/off or fails resolution.
    client_id: Option<String>,
}

/// Pure resolution/validation logic behind [`resolve_oidc_config`]. Split out
/// so tests can exercise flag/env precedence and the "half configured" error
/// cases without touching real process env vars.
///
/// Precedence per field: flag wins over env var. `Ok(None)` means OIDC is not
/// configured at all (loopback-only unauthenticated serving stays legal).
/// `Err` means OIDC was *partially* configured (an issuer with no audience,
/// or vice versa) -- that's always a mistake, regardless of bind host.
fn resolve_oidc_config_from(
    flag_issuer: Option<&str>,
    flag_audiences: &[String],
    env_issuer: Option<&str>,
    env_audience: Option<&str>,
    flag_client_id: Option<&str>,
    env_client_id: Option<&str>,
) -> StdResult<Option<ResolvedOidcConfig>, AppError> {
    let issuer = flag_issuer
        .map(str::to_string)
        .or_else(|| env_issuer.map(str::to_string))
        .filter(|s| !s.trim().is_empty());

    let audiences: Vec<String> = if !flag_audiences.is_empty() {
        flag_audiences
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        env_audience
            .map(|raw| {
                raw.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };

    // Optional even when OIDC is configured (see `ResolvedOidcConfig::client_id`
    // doc): flag wins over env, same precedence as issuer/audience, but never
    // participates in the "half configured" validation below.
    let client_id = flag_client_id
        .map(str::to_string)
        .or_else(|| env_client_id.map(str::to_string))
        .filter(|s| !s.trim().is_empty());

    match (issuer, audiences.is_empty()) {
        (None, true) => Ok(None),
        (Some(issuer), false) => Ok(Some(ResolvedOidcConfig {
            issuer,
            audiences,
            client_id,
        })),
        (Some(_), true) => Err(AppError::new(
            ErrorCategory::ValidationError,
            "NEWTON-SERVE-AUTH-003: --oidc-issuer (or NEWTON_OIDC_ISSUER) was set but no \
             --oidc-audience (or NEWTON_OIDC_AUDIENCE) was provided; OIDC requires at least one \
             accepted audience value"
                .to_string(),
        )
        .with_code("NEWTON-SERVE-AUTH-003")),
        (None, false) => Err(AppError::new(
            ErrorCategory::ValidationError,
            "NEWTON-SERVE-AUTH-003: --oidc-audience (or NEWTON_OIDC_AUDIENCE) was provided but \
             no --oidc-issuer (or NEWTON_OIDC_ISSUER) was set; OIDC requires an issuer URL"
                .to_string(),
        )
        .with_code("NEWTON-SERVE-AUTH-003")),
    }
}

/// Resolves the effective OIDC config for this `newton serve` invocation,
/// reading `NEWTON_OIDC_ISSUER` / `NEWTON_OIDC_AUDIENCE` as the env-var
/// fallback for the corresponding flags. See [`resolve_oidc_config_from`] for
/// the (unit-tested) precedence and validation logic.
fn resolve_oidc_config(args: &ServeArgs) -> StdResult<Option<ResolvedOidcConfig>, AppError> {
    let env_issuer = std::env::var("NEWTON_OIDC_ISSUER").ok();
    let env_audience = std::env::var("NEWTON_OIDC_AUDIENCE").ok();
    let env_client_id = std::env::var("NEWTON_OIDC_CLIENT_ID").ok();
    resolve_oidc_config_from(
        args.oidc_issuer.as_deref(),
        &args.oidc_audience,
        env_issuer.as_deref(),
        env_audience.as_deref(),
        args.oidc_client_id.as_deref(),
        env_client_id.as_deref(),
    )
}

/// Enforces the fail-closed exposure rule (audit finding C5): a non-loopback
/// `--host` bind is the operator's explicit opt-in to remote exposure, and
/// from this point on that opt-in is only legal when OIDC is configured --
/// `newton serve` must refuse to start otherwise, rather than boot wide open
/// behind a warning banner. A loopback bind stays optionally-authenticated
/// (friction-free local tool default); a loopback bind with no OIDC
/// configured still emits a low-severity tracing event so the unauthenticated
/// state is observable, without alarming operators for the (expected) common
/// case. Extracted from `serve()`'s body so the decision is unit-testable
/// without starting a real HTTP listener (spec 074 PR-6 / B5; extended here).
fn check_non_loopback_bind(
    host: &str,
    port: u16,
    oidc_configured: bool,
) -> StdResult<(), AppError> {
    let non_loopback_bind = !is_loopback_host(host);
    match (non_loopback_bind, oidc_configured) {
        (true, false) => Err(AppError::new(
            ErrorCategory::ValidationError,
            format!(
                "NEWTON-SERVE-AUTH-001: refusing to bind non-loopback host {host:?}:{port} \
                 without authentication configured. newton serve requires OIDC whenever --host \
                 is not a loopback address (127.0.0.1, ::1, or localhost): set --oidc-issuer (or \
                 the NEWTON_OIDC_ISSUER env var) and at least one --oidc-audience (or the \
                 comma-separated NEWTON_OIDC_AUDIENCE env var), or bind 127.0.0.1 for \
                 unauthenticated local-only use."
            ),
        )
        .with_code("NEWTON-SERVE-AUTH-001")),
        (true, true) => {
            tracing::info!(
                event = "non_loopback_bind_authenticated",
                host = %host,
                port = port,
                "newton serve is binding a non-loopback host; OIDC authentication is configured and enforced on the API"
            );
            Ok(())
        }
        (false, true) => {
            tracing::info!(
                event = "loopback_bind_authenticated",
                host = %host,
                port = port,
                "newton serve is binding a loopback host with OIDC authentication configured and enforced on the API"
            );
            Ok(())
        }
        (false, false) => {
            tracing::debug!(
                event = "unauthenticated_loopback",
                host = %host,
                port = port,
                "newton serve is binding a loopback host with no OIDC configured; the Newton HTTP API is unauthenticated (local-only default)"
            );
            Ok(())
        }
    }
}

/// Whether the REST API should advertise permissive cross-origin access
/// (`Access-Control-Allow-Origin: *`). Permissive CORS is only sound once the
/// API is authenticated (bearer-token OIDC): a separate-origin, token-bearing
/// frontend is a legitimate deployment, and an un-tokened cross-origin request
/// is rejected by the auth layer before it can read anything. On the default
/// *unauthenticated* (loopback) serve it must be OFF — otherwise any web page
/// could read API responses and drive state-changing endpoints (e.g. approving
/// a pending Human-in-the-Loop gate) cross-origin. The embedded SPA is served
/// same-origin from this listener, so it never needs a CORS grant. Pure so the
/// decision is unit-testable without starting a real HTTP listener (audit
/// finding newton-06).
fn permissive_cors_allowed(oidc_configured: bool) -> bool {
    oidc_configured
}

/// Builds the human-readable startup banner lines. Newton's `info!` startup
/// logs are silenced in the serve (Server) console context, and
/// cli-framework's `serve()` prints nothing, so without this `newton serve`
/// looks like it hangs with no output. Pure so the banner text — including
/// the auth status line and the non-loopback exposure note — can be unit
/// tested without starting a real HTTP listener.
///
/// `oidc_issuer` being `Some` means OIDC is configured and enforced;
/// `non_loopback_bind` being `true` implies `oidc_issuer.is_some()` by the
/// time this is called, because `check_non_loopback_bind` already refused to
/// start otherwise (audit finding C5) -- so there is no "unauthenticated and
/// exposed" state left to warn about here.
#[allow(clippy::too_many_arguments)]
fn startup_banner_lines(
    host: &str,
    port: u16,
    web_ui_mode: &str,
    with_mcp: bool,
    with_embedded_ailoop: bool,
    ailoop_base_path: &str,
    non_loopback_bind: bool,
    oidc_issuer: Option<&str>,
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
    match oidc_issuer {
        Some(issuer) => lines.push(format!("    Auth       OIDC required (issuer: {issuer})")),
        None => lines.push(
            "    Auth       disabled (no OIDC configured; loopback-only default)".to_string(),
        ),
    }
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
        lines.push(format!(
            "  Bound to non-loopback host \"{host}\" — reachable from other hosts on this interface."
        ));
        lines.push(
            "  OIDC authentication is REQUIRED and enforced on the API (see \"Auth\" above)."
                .to_string(),
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
    use cli_framework_oidc::server::{oidc_validation_layer, AudiencePolicy, OidcValidationConfig};
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

    let oidc_config = resolve_oidc_config(&args)?;
    check_non_loopback_bind(&args.host, args.port, oidc_config.is_some())?;
    let non_loopback_bind = !is_loopback_host(&args.host);

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

    let v1 = api::api_v1_router(state, args.with_magic_tools);

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
        .health_version(env!("CARGO_PKG_VERSION"));

    // CORS (audit finding newton-06): only advertise permissive cross-origin
    // access once the API is authenticated. The unauthenticated default serves
    // the embedded SPA same-origin and must not grant any web page read/drive
    // access to the API.
    if permissive_cors_allowed(oidc_config.is_some()) {
        builder = builder.cors(CorsLayer::permissive());
    }

    // OIDC auth (audit finding C5): gates the `/api/{version}` mounts,
    // non-primary `mount()`s (e.g. the embedded ailoop router), and `/mcp`.
    // It deliberately does NOT wrap `root_fallback` (the embedded SPA) or
    // `/healthz`/`/readyz` (we never call `.protect_health(true)`), so the
    // web UI keeps loading without a token while the API itself is gated.
    if let Some(ref oidc) = oidc_config {
        let audience_policy = if oidc.audiences.len() == 1 {
            AudiencePolicy::Require(oidc.audiences[0].clone())
        } else {
            AudiencePolicy::RequireAny(oidc.audiences.clone())
        };
        let oidc_cfg = OidcValidationConfig::new(oidc.issuer.clone(), audience_policy);
        let auth_layer = oidc_validation_layer(oidc_cfg).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("NEWTON-SERVE-AUTH-004: invalid OIDC configuration: {e}"),
            )
            .with_code("NEWTON-SERVE-AUTH-004")
        })?;
        builder = builder.auth(auth_layer);
        info!(
            event = "oidc_auth_enabled",
            issuer = %oidc.issuer,
            audience_count = oidc.audiences.len(),
            "OIDC authentication enforced on the Newton API"
        );
    }

    // Web UI: the embedded bundle is served at all non-API paths by default;
    // `--no-web` opts out (API only). The SPA can't reach `/api/**` before it
    // has a token, so the public `/auth-config` route (mounted alongside the
    // SPA fallback, NOT under `/api`) hands it the non-secret OIDC metadata
    // it needs to self-configure a login.
    let web_ui_mode: &str = if args.no_web {
        "disabled"
    } else {
        let public_auth_config = api::PublicAuthConfig {
            oidc: oidc_config.as_ref().map(|oidc| api::PublicOidcConfig {
                issuer: oidc.issuer.clone(),
                audience: oidc.audiences[0].clone(),
                client_id: oidc.client_id.clone(),
            }),
        };
        builder = builder.root_fallback(api::embedded_web_router(public_auth_config));
        "embedded"
    };
    info!(
        event = "web_ui",
        mode = web_ui_mode,
        "web UI serving mode resolved"
    );

    if args.with_mcp {
        let ctx = crate::cli::context::NewtonContext::new();
        let mcp_router =
            crate::cli::framework_setup::build_mcp_router_for_serve(ctx).map_err(|err| {
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
        oidc_config.as_ref().map(|c| c.issuer.as_str()),
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
mod check_non_loopback_bind_tests {
    use super::*;

    // loopback + no OIDC -> allowed, runs unauthenticated (today's default).
    #[test]
    fn loopback_without_oidc_is_allowed() {
        assert!(check_non_loopback_bind("127.0.0.1", 3000, false).is_ok());
        assert!(check_non_loopback_bind("localhost", 3000, false).is_ok());
        assert!(check_non_loopback_bind("::1", 3000, false).is_ok());
    }

    // loopback + OIDC configured -> allowed, auth enforced anyway.
    #[test]
    fn loopback_with_oidc_is_allowed() {
        assert!(check_non_loopback_bind("127.0.0.1", 3000, true).is_ok());
        assert!(check_non_loopback_bind("localhost", 3000, true).is_ok());
    }

    // non-loopback + OIDC configured -> allowed (the only way to expose the API).
    #[test]
    fn non_loopback_with_oidc_is_allowed() {
        assert!(check_non_loopback_bind("0.0.0.0", 3000, true).is_ok());
        assert!(check_non_loopback_bind("203.0.113.5", 8080, true).is_ok());
    }

    // non-loopback + no OIDC -> refused, with an actionable error naming the
    // exact flags/env vars the operator needs to set (audit finding C5).
    #[test]
    fn non_loopback_without_oidc_is_refused() {
        let err = check_non_loopback_bind("0.0.0.0", 3000, false).unwrap_err();
        assert!(
            err.message.contains("NEWTON-SERVE-AUTH-001"),
            "err={}",
            err.message
        );
        assert!(err.message.contains("--oidc-issuer"), "err={}", err.message);
        assert!(
            err.message.contains("NEWTON_OIDC_ISSUER"),
            "err={}",
            err.message
        );
        assert!(
            err.message.contains("--oidc-audience"),
            "err={}",
            err.message
        );
        assert!(
            err.message.contains("NEWTON_OIDC_AUDIENCE"),
            "err={}",
            err.message
        );
        assert!(err.message.contains("0.0.0.0"), "err={}", err.message);

        let err2 = check_non_loopback_bind("203.0.113.5", 8080, false).unwrap_err();
        assert!(
            err2.message.contains("NEWTON-SERVE-AUTH-001"),
            "err={}",
            err2.message
        );
    }
}

#[cfg(test)]
mod permissive_cors_allowed_tests {
    use super::*;

    #[test]
    fn permissive_cors_off_when_unauthenticated() {
        // newton-06: the default unauthenticated serve must not advertise
        // permissive cross-origin access.
        assert!(!permissive_cors_allowed(false));
    }

    #[test]
    fn permissive_cors_on_when_oidc_configured() {
        // Retained for authenticated (bearer-token) deployments with a
        // separate-origin frontend.
        assert!(permissive_cors_allowed(true));
    }
}

#[cfg(test)]
mod resolve_oidc_config_tests {
    use super::*;

    #[test]
    fn nothing_configured_is_none() {
        let cfg = resolve_oidc_config_from(None, &[], None, None, None, None).unwrap();
        assert!(cfg.is_none());
    }

    #[test]
    fn flag_issuer_and_single_flag_audience_configures_require() {
        let cfg = resolve_oidc_config_from(
            Some("https://issuer.example.com"),
            &["newton-api".to_string()],
            None,
            None,
            None,
            None,
        )
        .unwrap()
        .expect("configured");
        assert_eq!(cfg.issuer, "https://issuer.example.com");
        assert_eq!(cfg.audiences, vec!["newton-api".to_string()]);
        assert_eq!(cfg.client_id, None);
    }

    #[test]
    fn multiple_flag_audiences_are_all_kept_for_require_any() {
        let cfg = resolve_oidc_config_from(
            Some("https://issuer.example.com"),
            &["aud-a".to_string(), "aud-b".to_string()],
            None,
            None,
            None,
            None,
        )
        .unwrap()
        .expect("configured");
        assert_eq!(
            cfg.audiences,
            vec!["aud-a".to_string(), "aud-b".to_string()]
        );
    }

    #[test]
    fn env_vars_are_used_when_flags_absent() {
        let cfg = resolve_oidc_config_from(
            None,
            &[],
            Some("https://issuer.example.com"),
            Some("aud-a,aud-b"),
            None,
            None,
        )
        .unwrap()
        .expect("configured");
        assert_eq!(cfg.issuer, "https://issuer.example.com");
        assert_eq!(
            cfg.audiences,
            vec!["aud-a".to_string(), "aud-b".to_string()]
        );
    }

    #[test]
    fn env_audience_list_trims_whitespace_and_drops_empties() {
        let cfg = resolve_oidc_config_from(
            None,
            &[],
            Some("https://issuer.example.com"),
            Some(" aud-a , aud-b ,,"),
            None,
            None,
        )
        .unwrap()
        .expect("configured");
        assert_eq!(
            cfg.audiences,
            vec!["aud-a".to_string(), "aud-b".to_string()]
        );
    }

    #[test]
    fn flag_takes_precedence_over_env_for_issuer() {
        let cfg = resolve_oidc_config_from(
            Some("https://flag-issuer.example.com"),
            &["aud".to_string()],
            Some("https://env-issuer.example.com"),
            Some("env-aud"),
            None,
            None,
        )
        .unwrap()
        .expect("configured");
        assert_eq!(cfg.issuer, "https://flag-issuer.example.com");
        assert_eq!(cfg.audiences, vec!["aud".to_string()]);
    }

    #[test]
    fn flag_audience_takes_precedence_over_env_audience() {
        let cfg = resolve_oidc_config_from(
            Some("https://issuer.example.com"),
            &["flag-aud".to_string()],
            None,
            Some("env-aud"),
            None,
            None,
        )
        .unwrap()
        .expect("configured");
        assert_eq!(cfg.audiences, vec!["flag-aud".to_string()]);
    }

    #[test]
    fn issuer_without_audience_is_rejected() {
        let err = resolve_oidc_config_from(
            Some("https://issuer.example.com"),
            &[],
            None,
            None,
            None,
            None,
        )
        .unwrap_err();
        assert!(
            err.message.contains("NEWTON-SERVE-AUTH-003"),
            "err={}",
            err.message
        );
        assert!(
            err.message.contains("--oidc-audience"),
            "err={}",
            err.message
        );
    }

    #[test]
    fn audience_without_issuer_is_rejected() {
        let err = resolve_oidc_config_from(None, &["aud".to_string()], None, None, None, None)
            .unwrap_err();
        assert!(
            err.message.contains("NEWTON-SERVE-AUTH-003"),
            "err={}",
            err.message
        );
        assert!(err.message.contains("--oidc-issuer"), "err={}", err.message);
    }

    #[test]
    fn client_id_is_none_when_not_configured() {
        let cfg = resolve_oidc_config_from(
            Some("https://issuer.example.com"),
            &["newton-api".to_string()],
            None,
            None,
            None,
            None,
        )
        .unwrap()
        .expect("configured");
        assert_eq!(cfg.client_id, None);
    }

    #[test]
    fn flag_client_id_is_captured() {
        let cfg = resolve_oidc_config_from(
            Some("https://issuer.example.com"),
            &["newton-api".to_string()],
            None,
            None,
            Some("newton-spa"),
            None,
        )
        .unwrap()
        .expect("configured");
        assert_eq!(cfg.client_id, Some("newton-spa".to_string()));
    }

    #[test]
    fn env_client_id_is_used_when_flag_absent() {
        let cfg = resolve_oidc_config_from(
            Some("https://issuer.example.com"),
            &["newton-api".to_string()],
            None,
            None,
            None,
            Some("env-spa"),
        )
        .unwrap()
        .expect("configured");
        assert_eq!(cfg.client_id, Some("env-spa".to_string()));
    }

    #[test]
    fn flag_client_id_takes_precedence_over_env() {
        let cfg = resolve_oidc_config_from(
            Some("https://issuer.example.com"),
            &["newton-api".to_string()],
            None,
            None,
            Some("flag-spa"),
            Some("env-spa"),
        )
        .unwrap()
        .expect("configured");
        assert_eq!(cfg.client_id, Some("flag-spa".to_string()));
    }

    #[test]
    fn client_id_without_issuer_or_audience_does_not_enable_oidc() {
        // --oidc-client-id is meaningless on its own; OIDC stays off, and it
        // must never be required (that would break API-only deployments).
        let cfg =
            resolve_oidc_config_from(None, &[], None, None, Some("orphan-spa"), None).unwrap();
        assert!(cfg.is_none());
    }
}

#[cfg(test)]
mod startup_banner_tests {
    use super::*;

    #[test]
    fn loopback_bind_without_oidc_shows_auth_disabled_and_no_exposure_note() {
        let lines = startup_banner_lines(
            "127.0.0.1",
            3000,
            "embedded",
            false,
            false,
            "/ailoop",
            false,
            None,
        );
        let joined = lines.join("\n");
        assert!(joined.contains("Newton serving on http://127.0.0.1:3000"));
        assert!(joined.contains("Web UI     http://127.0.0.1:3000/"));
        assert!(joined.contains("Auth       disabled"));
        assert!(!joined.contains("is not yet implemented"));
        assert!(!joined.contains("REQUIRED"));
    }

    #[test]
    fn loopback_bind_with_oidc_shows_auth_required_line() {
        let lines = startup_banner_lines(
            "127.0.0.1",
            3000,
            "embedded",
            false,
            false,
            "/ailoop",
            false,
            Some("https://issuer.example.com"),
        );
        let joined = lines.join("\n");
        assert!(
            joined.contains("Auth       OIDC required (issuer: https://issuer.example.com)"),
            "{joined}"
        );
        assert!(!joined.contains("is not yet implemented"));
    }

    #[test]
    fn non_loopback_bind_notes_exposure_and_enforced_auth_without_lying() {
        let lines = startup_banner_lines(
            "0.0.0.0",
            3000,
            "embedded",
            false,
            false,
            "/ailoop",
            true,
            Some("https://issuer.example.com"),
        );
        let joined = lines.join("\n");
        assert!(
            joined.contains("Bound to non-loopback host \"0.0.0.0\""),
            "{joined}"
        );
        assert!(joined.contains("REQUIRED and enforced"), "{joined}");
        // The old "authentication is not yet implemented" lie must be gone.
        assert!(!joined.contains("is not yet implemented"));
        assert!(!joined.contains("deferred to a future spec"));
        assert!(!joined.contains("UNAUTHENTICATED"));
    }

    #[test]
    fn unspecified_bind_addresses_map_to_browsable_loopback() {
        let lines = startup_banner_lines(
            "0.0.0.0", 3000, "embedded", false, false, "/ailoop", false, None,
        );
        assert!(lines.iter().any(|l| l.contains("http://127.0.0.1:3000")));

        let lines =
            startup_banner_lines("::", 3000, "embedded", false, false, "/ailoop", false, None);
        assert!(lines.iter().any(|l| l.contains("http://127.0.0.1:3000")));

        let lines = startup_banner_lines(
            "[::]", 3000, "embedded", false, false, "/ailoop", false, None,
        );
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
            None,
        );
        let joined = lines.join("\n");
        assert!(!joined.contains("Web UI"));
        assert!(joined.contains("(web UI disabled via --no-web)"));
    }

    #[test]
    fn mcp_enabled_adds_mcp_line() {
        let lines = startup_banner_lines(
            "127.0.0.1",
            3000,
            "embedded",
            true,
            false,
            "/ailoop",
            false,
            None,
        );
        assert!(lines
            .iter()
            .any(|l| l.contains("MCP        http://127.0.0.1:3000/mcp")));
    }

    #[test]
    fn ailoop_enabled_adds_ailoop_line_with_base_path() {
        let lines = startup_banner_lines(
            "127.0.0.1",
            3000,
            "embedded",
            false,
            true,
            "/hil",
            false,
            None,
        );
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
            None,
        );
        assert!(lines.iter().any(|l| l.contains("Press Ctrl+C to stop.")));
    }
}
