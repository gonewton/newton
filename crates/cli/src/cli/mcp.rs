//! MCP-mode wiring for Newton CLI (issue #237).
//!
//! When the user passes `newton mcp serve`, Newton short-circuits subcommand
//! dispatch and starts the cli-framework MCP HTTP server. cli-framework owns
//! the protocol; Newton's contribution is:
//!
//! 1. A pre-bind probe that emits a single structured `tracing::info!` event
//!    after we have proven the host:port is bindable.
//! 2. Mapping cli-framework errors onto stable Newton error codes
//!    `NEWTON-MCP-001` (bind failure) and `NEWTON-MCP-002` (upstream runtime
//!    error after a successful bind).
use crate::cli::framework_setup::{error_codes, MCP_EXPOSED_COMMAND_IDS};

/// Newton's documented MCP defaults (spec §4.2). cli-framework currently
/// defaults `--mcp-port` to `8080`; Newton overrides to `8730` to avoid
/// clashing with `newton serve`.
pub const DEFAULT_MCP_HOST: &str = "127.0.0.1";
pub const DEFAULT_MCP_PORT: u16 = 8730;
pub const DEFAULT_MCP_PATH: &str = "/mcp";

/// Parsed MCP CLI flags.
#[derive(Debug, Clone)]
pub struct McpFlags {
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl Default for McpFlags {
    fn default() -> Self {
        Self {
            host: DEFAULT_MCP_HOST.to_string(),
            port: DEFAULT_MCP_PORT,
            path: DEFAULT_MCP_PATH.to_string(),
        }
    }
}

/// Returns true iff argv matches the subcommand form: argv[1]=="mcp" && argv[2]=="serve".
/// argv[0] is the binary name; elements beyond index 2 are ignored (they are flags).
pub fn is_mcp_subcommand(argv: &[String]) -> bool {
    argv.get(1).map(|s| s == "mcp").unwrap_or(false)
        && argv.get(2).map(|s| s == "serve").unwrap_or(false)
}

/// Parse `--host`, `--port`, `--path` (space- or `=`-separated) from argv.
/// Unknown values fall back to Newton defaults.
pub fn parse_mcp_flags(argv: &[String]) -> McpFlags {
    let mut flags = McpFlags::default();
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        // Short forms (--host/--port/--path): used by `mcp serve` subcommand path.
        if a == "--host" && i + 1 < argv.len() {
            flags.host = argv[i + 1].clone();
            i += 2;
            continue;
        }
        if let Some(v) = a.strip_prefix("--host=") {
            flags.host = v.to_string();
            i += 1;
            continue;
        }
        if a == "--port" && i + 1 < argv.len() {
            if let Ok(p) = argv[i + 1].parse::<u16>() {
                flags.port = p;
            }
            i += 2;
            continue;
        }
        if let Some(v) = a.strip_prefix("--port=") {
            if let Ok(p) = v.parse::<u16>() {
                flags.port = p;
            }
            i += 1;
            continue;
        }
        if a == "--path" && i + 1 < argv.len() {
            flags.path = argv[i + 1].clone();
            i += 2;
            continue;
        }
        if let Some(v) = a.strip_prefix("--path=") {
            flags.path = v.to_string();
            i += 1;
            continue;
        }
        i += 1;
    }
    flags
}

/// Returns the number of Newton commands exposed as MCP tools under the
/// ExposeMcpOnly policy (issue #309).
pub fn tool_count() -> usize {
    MCP_EXPOSED_COMMAND_IDS.len()
}

/// Build the argv that cli-framework expects: ensure host/port/path flags are
/// present (with Newton defaults applied when absent) so the framework's
/// `extract_mcp_args_from_raw` honours our overrides.
pub fn argv_with_newton_defaults(argv: &[String], flags: &McpFlags) -> Vec<String> {
    fn has(out: &[String], needle: &str) -> bool {
        let prefix = format!("{}=", needle);
        out.iter().any(|a| a == needle || a.starts_with(&prefix))
    }
    let mut out: Vec<String> = argv.to_vec();
    if !has(&out, "--host") {
        out.push("--host".to_string());
        out.push(flags.host.clone());
    }
    if !has(&out, "--port") {
        out.push("--port".to_string());
        out.push(flags.port.to_string());
    }
    if !has(&out, "--path") {
        out.push("--path".to_string());
        out.push(flags.path.clone());
    }
    out
}

/// Probe-bind `host:port` to fail-fast on conflicts before the framework
/// starts up. The listener is dropped immediately; cli-framework will rebind
/// when it owns the runtime. The TOCTOU window is acceptable for the
/// `NEWTON-MCP-001` policy (spec §4.3).
pub async fn probe_bind(flags: &McpFlags) -> Result<(), std::io::Error> {
    let addr = format!("{}:{}", flags.host, flags.port);
    let l = tokio::net::TcpListener::bind(&addr).await?;
    drop(l);
    Ok(())
}

/// Run MCP mode using cli-framework's `serve_mcp` entry point. Returns the
/// process exit code; the caller in `main.rs` terminates the process with it
/// (the only place outside this MCP-mode short-circuit permitted to do so).
pub async fn run(argv: Vec<String>, ctx: crate::cli::context::NewtonContext) -> i32 {
    let flags = parse_mcp_flags(&argv);
    let bind_address = format!("{}:{}", flags.host, flags.port);

    if let Err(e) = probe_bind(&flags).await {
        eprintln!(
            "{}: failed to bind MCP server to {}: {}",
            error_codes::NEWTON_MCP_001,
            bind_address,
            e
        );
        return 1;
    }

    let count = tool_count();
    tracing::info!(
        event = "mcp_serve_started",
        mcp_enabled = true,
        bind_address = %bind_address,
        mcp_path = %flags.path,
        tool_count = count,
        "MCP server starting"
    );
    // Mirror the structured event to stderr as a single JSON line. The
    // file-based tracing layer writes to disk under `--log-dir`, but
    // integration tests need a direct, deterministic surface. Spec §4.6.
    eprintln!(
        "{{\"event\":\"mcp_serve_started\",\"mcp_enabled\":true,\"bind_address\":\"{}\",\"mcp_path\":\"{}\",\"tool_count\":{}}}",
        bind_address, flags.path, count
    );

    let app = match crate::cli::framework_setup::build_app(ctx) {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "{}: failed to construct MCP command registry: {}",
                error_codes::NEWTON_MCP_002,
                e
            );
            return 1;
        }
    };

    // Hand off to cli-framework. We pass the original argv so the framework's
    // own `--mcp-serve` short-circuit fires inside `run_with_args`.
    let mut app = app;
    let argv_for_framework = argv_with_newton_defaults(&argv, &flags);
    match app.run_with_args(argv_for_framework).await {
        Ok(()) => 0,
        Err(e) => {
            // Bind-failure surfaces as anyhow; map back to NEWTON-MCP-001 so
            // the test harness sees a stable code on stderr.
            let msg = format!("{:#}", e);
            if msg.contains("MCP_BIND_FAILED") || msg.contains("address") && msg.contains("in use")
            {
                eprintln!(
                    "{}: failed to bind MCP server to {}: {}",
                    error_codes::NEWTON_MCP_001,
                    bind_address,
                    msg
                );
            } else {
                eprintln!(
                    "{}: MCP runtime error: {}",
                    error_codes::NEWTON_MCP_002,
                    msg
                );
            }
            1
        }
    }
}
