//! Issue #309: verify that ExposeMcpOnly policy produces exactly the six
//! expected tool ids and excludes the eight non-exposed command ids.
use cli_framework::command::registry::CommandRegistry;
use cli_framework::mcp::{McpToolExportPolicy, McpToolRegistry};
use newton_cli::cli::framework_setup::{enumerate_commands, MCP_EXPOSED_COMMAND_IDS};

fn build_exposed_registry() -> McpToolRegistry {
    let mut registry = CommandRegistry::new();
    for cmd in enumerate_commands() {
        registry.register(cmd);
    }
    McpToolRegistry::from_command_registry_with_policy(
        &registry,
        "newton",
        McpToolExportPolicy::ExposeMcpOnly,
    )
}

#[test]
fn expose_mcp_only_tool_count_equals_allowlist() {
    let registry = build_exposed_registry();
    assert_eq!(
        registry.tool_count(),
        MCP_EXPOSED_COMMAND_IDS.len(),
        "ExposeMcpOnly policy should expose exactly {} tools",
        MCP_EXPOSED_COMMAND_IDS.len()
    );
}

#[test]
fn expose_mcp_only_includes_allowed_tools() {
    let registry = build_exposed_registry();
    for id in MCP_EXPOSED_COMMAND_IDS {
        let tool_name = format!("newton.{}", id);
        assert!(
            registry.resolve_tool(&tool_name).is_some(),
            "expected tool '{}' to be in MCP registry",
            tool_name
        );
    }
}

#[test]
fn expose_mcp_only_excludes_non_allowed_tools() {
    let registry = build_exposed_registry();
    for id in &[
        "init",
        "batch",
        "serve",
        "checkpoint",
        "artifact",
        "webhook",
        "doctor",
        "completion",
    ] {
        let tool_name = format!("newton.{}", id);
        assert!(
            registry.resolve_tool(&tool_name).is_none(),
            "expected tool '{}' to NOT be in MCP registry",
            tool_name
        );
    }
}
