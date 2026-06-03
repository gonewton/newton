//! Issue #237: MCP `tools/list` exposes the same set of commands Newton
//! registers via `build_app`, and `tools/call` for `health` reaches the same
//! handler as `newton health`. The framework owns the JSON-RPC transport;
//! Newton's responsibility is to make sure (a) the registry is the single
//! source of truth and (b) `health` is in it.
//!
//! A full HTTP round-trip against the streamable HTTP transport requires
//! session negotiation (init → tools/list → tools/call) that mirrors
//! `cli-framework/tests/integration/mcp_http.rs`. We exercise the contract
//! at the registry level here to keep the test deterministic and avoid
//! flakiness from the long-running HTTP server.
use newton_cli::cli::framework_setup::{enumerate_commands, REGISTERED_COMMAND_IDS};
use newton_cli::cli::mcp;

#[test]
fn health_is_in_registered_commands() {
    assert!(
        REGISTERED_COMMAND_IDS.contains(&"health"),
        "health must be in REGISTERED_COMMAND_IDS so MCP exports it as a tool"
    );
}

#[test]
fn enumerate_commands_includes_health_with_spec() {
    let cmds = enumerate_commands();
    let health = cmds
        .iter()
        .find(|c| c.id.as_ref() == "health")
        .expect("health command registered");
    // CommandSpec is what cli-framework's MCP layer translates into a JSON
    // Schema; all commands now carry a mandatory spec.
    assert!(
        !health.spec.summary.is_empty(),
        "health command should expose a CommandSpec for MCP tool derivation"
    );
}

#[test]
fn argv_with_newton_defaults_inserts_missing_flags() {
    let argv: Vec<String> = vec!["newton".into(), "--mcp-serve".into()];
    let flags = mcp::parse_mcp_flags(&argv);
    let out = mcp::argv_with_newton_defaults(&argv, &flags);
    assert!(out.iter().any(|a| a == "--mcp-host"));
    assert!(out.iter().any(|a| a == "--mcp-port"));
    assert!(out.iter().any(|a| a == "--mcp-path"));
    // Newton default port is 8730 (spec §4.2), distinct from cli-framework's
    // upstream default of 8080.
    assert!(
        out.iter().any(|a| a == "8730"),
        "expected Newton default port 8730 in argv, got {:?}",
        out
    );
}

#[test]
fn argv_with_newton_defaults_preserves_user_overrides() {
    let argv: Vec<String> = vec![
        "newton".into(),
        "--mcp-serve".into(),
        "--mcp-port".into(),
        "9100".into(),
        "--mcp-host".into(),
        "0.0.0.0".into(),
    ];
    let flags = mcp::parse_mcp_flags(&argv);
    assert_eq!(flags.port, 9100);
    assert_eq!(flags.host, "0.0.0.0");
    let out = mcp::argv_with_newton_defaults(&argv, &flags);
    // User-supplied `--mcp-port 9100` MUST not be duplicated.
    let port_count = out.iter().filter(|a| *a == "--mcp-port").count();
    assert_eq!(port_count, 1);
}
