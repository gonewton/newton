use std::sync::Arc;

use anyhow::anyhow;
use cli_framework::command::registry::CommandRegistry;
use cli_framework::mcp::{
    CliFrameworkHandler, McpToolExportPolicy, McpToolRegistry, McpTransportKind,
};
use cli_framework::spec::command_tree::{CommandPath, GroupMetadata};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};

use crate::cli::args::DataVerb;
use crate::cli::context::NewtonContext;

use super::commands;
use super::error_codes;

/// Build an `axum::Router` that mounts the cli-framework MCP HTTP transport
/// under `mcp_path` on the caller-owned listener (issue #294).
///
/// This is the Newton-side adapter for what `aroff/cli-framework#29` aims to
/// expose upstream. Until the upstream `App::into_mcp_router(...)` lands,
/// Newton constructs the equivalent registry/service/router stack directly
/// from cli-framework's public MCP primitives. If the upstream API later
/// becomes available the implementation switches to that without changing the
/// public function signature here.
///
/// Returns:
/// * `NEWTON-SERVE-MCP-003` — required upstream MCP-mount API not available
///   in the linked cli-framework version.
/// * `NEWTON-SERVE-MCP-004` — cli-framework returned an error while building
///   the registry, the tool registry, or the HTTP service.
pub fn build_mcp_router_for_serve(
    _ctx: NewtonContext,
    mcp_path: &str,
) -> anyhow::Result<axum::Router> {
    let registry = build_mcp_command_registry()
        .map_err(|e| anyhow!("{}: {e}", error_codes::NEWTON_SERVE_MCP_004))?;

    let tool_registry = Arc::new(McpToolRegistry::from_command_registry_with_policy(
        &registry,
        "newton",
        McpToolExportPolicy::ExposeMcpOnly,
    ));
    if tool_registry.tool_count() == 0 {
        return Err(anyhow!(
            "{}: cli-framework returned an empty MCP tool registry",
            error_codes::NEWTON_SERVE_MCP_004
        ));
    }

    let session_manager = Arc::new(LocalSessionManager::default());
    let config = StreamableHttpServerConfig::default();
    let service = StreamableHttpService::new(
        {
            let tool_registry = Arc::clone(&tool_registry);
            move || {
                Ok(CliFrameworkHandler::new(
                    Arc::clone(&tool_registry),
                    McpTransportKind::Http,
                ))
            }
        },
        session_manager,
        config,
    );

    Ok(axum::Router::new().nest_service(mcp_path, service))
}

/// Build the full tree `CommandRegistry` used for MCP tool registration and
/// the `newton serve --with-mcp` router.  Both `build_app` and
/// `build_mcp_router_for_serve` derive their registrations from this function.
pub fn build_mcp_command_registry() -> anyhow::Result<CommandRegistry> {
    let mut registry = CommandRegistry::new();

    for cmd in super::all_root_commands() {
        registry.register(cmd);
    }

    let data_path = CommandPath::new(&["data"]).map_err(|e| anyhow!("CLI-PATH-001: {e}"))?;
    registry
        .register_group(
            &data_path,
            GroupMetadata {
                summary: "Catalog CRUD via HTTP-style verbs (get/post/put/patch/delete)",
                hidden: false,
            },
        )
        .map_err(|e| anyhow!("{e}"))?;

    for verb in [
        DataVerb::Get,
        DataVerb::Post,
        DataVerb::Put,
        DataVerb::Patch,
        DataVerb::Delete,
    ] {
        let path =
            CommandPath::new(&["data", verb.as_str()]).map_err(|e| anyhow!("CLI-PATH-001: {e}"))?;
        registry
            .register_at(&path, commands::data::data_verb_command(verb))
            .map_err(|e| anyhow!("{e}"))?;
    }

    Ok(registry)
}
