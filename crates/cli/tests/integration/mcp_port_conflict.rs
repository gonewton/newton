//! Issue #237: bind failure on `mcp serve --host:--port` MUST exit non-zero with
//! a single error line referencing `NEWTON-MCP-001` and the failed `host:port`
//! (spec §4.3).
use std::process::{Command, Stdio};

#[test]
fn port_conflict_emits_newton_mcp_001_and_exits_nonzero() {
    // Hold the port for the duration of the test.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();

    let bin = assert_cmd::cargo::cargo_bin("newton");
    let output = Command::new(bin)
        .arg("mcp")
        .arg("serve")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn newton mcp serve");

    drop(listener);

    assert!(
        !output.status.success(),
        "expected non-zero exit, got status={:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("NEWTON-MCP-001"),
        "stderr missing NEWTON-MCP-001: {}",
        stderr
    );
    assert!(
        stderr.contains(&format!("127.0.0.1:{}", port)),
        "stderr missing host:port {}: {}",
        port,
        stderr
    );
}
