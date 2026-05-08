//! Issue #237: MCP-mode startup emits a single structured log line containing
//! `event="mcp_serve_started"`, `mcp_enabled=true`, `bind_address`, `mcp_path`
//! and an integer `tool_count` matching `REGISTERED_COMMAND_IDS.len()` (+1
//! when the optional `ask` feature is enabled).
//!
//! We verify the contract end-to-end by spawning the binary on a free port
//! and reading the JSON-line we mirror to stderr (spec §4.6). The cli-framework
//! HTTP transport keeps running until killed; we read one line then SIGKILL.
use newton_cli::cli::framework_setup::REGISTERED_COMMAND_IDS;
use newton_cli::cli::mcp;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn pick_free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().unwrap().port()
}

#[test]
fn tool_count_matches_registered_command_ids() {
    let expected = REGISTERED_COMMAND_IDS.len() + if cfg!(feature = "ask") { 1 } else { 0 };
    assert_eq!(mcp::tool_count(), expected);
}

#[test]
fn mcp_serve_emits_structured_startup_log() {
    let port = pick_free_port();
    let bin = assert_cmd::cargo::cargo_bin("newton");
    let mut child = Command::new(bin)
        .arg("--mcp-serve")
        .arg("--mcp-host")
        .arg("127.0.0.1")
        .arg("--mcp-port")
        .arg(port.to_string())
        .arg("--mcp-path")
        .arg("/mcp")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn newton --mcp-serve");

    let stderr = child.stderr.take().expect("stderr pipe");
    let mut reader = BufReader::new(stderr);

    // Poll for the structured log line; cli-framework starts an Axum runtime
    // so we cannot wait_for_exit. Cap at 10s.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut found: Option<String> = None;
    while Instant::now() < deadline {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.contains("\"event\":\"mcp_serve_started\"") {
                    found = Some(line);
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    let line = found.expect("startup log line not observed within 10s");
    assert!(line.contains("\"mcp_enabled\":true"), "line={}", line);
    assert!(
        line.contains(&format!("\"bind_address\":\"127.0.0.1:{}\"", port)),
        "line={}",
        line
    );
    assert!(line.contains("\"mcp_path\":\"/mcp\""), "line={}", line);
    let expected_count = REGISTERED_COMMAND_IDS.len() + if cfg!(feature = "ask") { 1 } else { 0 };
    assert!(
        line.contains(&format!("\"tool_count\":{}", expected_count)),
        "line={}",
        line
    );
}
