//! Verifies that normal Newton invocations do not trigger MCP mode
//! and that `newton --help` does not emit the `mcp_serve_started` log line.
use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;

#[test]
fn help_does_not_emit_mcp_startup_log() {
    Command::cargo_bin("newton")
        .expect("binary should build")
        .arg("--help")
        .assert()
        .stderr(contains("mcp_serve_started").not())
        .stdout(contains("NEWTON-MCP-").not());
}
