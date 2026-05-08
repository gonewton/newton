//! Issue #237: when `--mcp-serve` is absent, Newton's behaviour MUST be
//! identical to the pre-change baseline. We verify two things here without
//! actually starting `newton serve` (which requires SQLite + a long-running
//! Axum process):
//!
//! 1. The MCP detector returns `false` for representative non-MCP argvs.
//! 2. `newton --help` exits successfully and emits **no**
//!    `mcp_serve_started` log line on stderr.
use assert_cmd::Command;
use newton_cli::cli::mcp;
use predicates::prelude::*;
use predicates::str::contains;

#[test]
fn is_mcp_serve_returns_false_for_normal_argv() {
    let argv: Vec<String> = ["newton", "serve", "--port", "8080"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(!mcp::is_mcp_serve(&argv));
}

#[test]
fn is_mcp_serve_returns_false_for_run_with_mcp_in_unrelated_value() {
    // Make sure we don't get fooled by `--mcp-host=foo` without `--mcp-serve`.
    let argv: Vec<String> = ["newton", "run", "--workspace", "--mcp-host", "x"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(!mcp::is_mcp_serve(&argv));
}

#[test]
fn help_does_not_emit_mcp_startup_log() {
    Command::cargo_bin("newton")
        .expect("binary should build")
        .arg("--help")
        .assert()
        .stderr(contains("mcp_serve_started").not())
        .stdout(contains("NEWTON-MCP-").not());
}
