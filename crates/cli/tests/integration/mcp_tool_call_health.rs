//! Issue #237: MCP `tools/list` exposes the same set of commands Newton
//! registers via `build_app`. The `health` command has been removed; this
//! test verifies it is NOT present in the MCP registry.
use newton_cli::cli::framework_setup::{enumerate_commands, REGISTERED_COMMAND_IDS};
use newton_cli::cli::mcp;

#[test]
fn health_is_not_in_registered_commands() {
    assert!(
        !REGISTERED_COMMAND_IDS.contains(&"health"),
        "health must NOT be in REGISTERED_COMMAND_IDS — it has been removed"
    );
}

#[test]
fn enumerate_commands_does_not_include_health() {
    let cmds = enumerate_commands();
    assert!(
        cmds.iter().all(|c| c.id.as_ref() != "health"),
        "health command must not be registered after removal"
    );
}

#[test]
fn argv_with_newton_defaults_inserts_missing_flags() {
    let argv: Vec<String> = vec!["newton".into(), "mcp".into(), "serve".into()];
    let flags = mcp::parse_mcp_flags(&argv);
    let out = mcp::argv_with_newton_defaults(&argv, &flags);
    assert!(out.iter().any(|a| a == "--host"));
    assert!(out.iter().any(|a| a == "--port"));
    assert!(out.iter().any(|a| a == "--path"));
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
        "mcp".into(),
        "serve".into(),
        "--port".into(),
        "9100".into(),
        "--host".into(),
        "0.0.0.0".into(),
    ];
    let flags = mcp::parse_mcp_flags(&argv);
    assert_eq!(flags.port, 9100);
    assert_eq!(flags.host, "0.0.0.0");
    let out = mcp::argv_with_newton_defaults(&argv, &flags);
    // User-supplied `--port 9100` MUST not be duplicated.
    let port_count = out.iter().filter(|a| *a == "--port").count();
    assert_eq!(port_count, 1);
}
