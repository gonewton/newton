//! Issue #309 / #336: verify that ExposeMcpOnly policy produces exactly the nine
//! expected tool ids and excludes non-exposed command ids.
use cli_framework::mcp::{McpToolExportPolicy, McpToolRegistry};
use newton_cli::cli::framework_setup::{build_mcp_command_registry, MCP_EXPOSED_COMMAND_IDS};

fn build_exposed_registry() -> McpToolRegistry {
    let registry =
        build_mcp_command_registry().expect("failed to build command registry for MCP test");
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
        let tool_name = format!("newton_{}", id.replace('.', "_"));
        assert!(
            registry.resolve_tool(&tool_name).is_some(),
            "expected tool '{}' to be in MCP registry",
            tool_name
        );
    }
}

#[test]
fn expose_mcp_only_newton_data_group_is_not_a_tool() {
    // The `data` node is a group, not a leaf — it must NOT appear as a tool.
    let registry = build_exposed_registry();
    assert!(
        registry.resolve_tool("newton_data").is_none(),
        "expected tool 'newton_data' to NOT be in MCP registry (it is a group, not a leaf)"
    );
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
        // "completion" — now framework-provided, not in newton's CommandRegistry
        // "data" removed — it is now a group node, not a tool
    ] {
        let tool_name = format!("newton_{}", id);
        assert!(
            registry.resolve_tool(&tool_name).is_none(),
            "expected tool '{}' to NOT be in MCP registry",
            tool_name
        );
    }
}
