//! Integration tests for `newton mcp serve` subcommand:
//! - emits `mcp_serve_started` JSON on stderr
//! - on an occupied port emits `NEWTON-MCP-001`
//! - `tool_count` matches `MCP_EXPOSED_COMMAND_IDS.len()`
use newton_cli::cli::framework_setup::MCP_EXPOSED_COMMAND_IDS;
use newton_cli::cli::mcp;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn pick_free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().unwrap().port()
}

#[test]
fn is_mcp_subcommand_detects_subcommand_form() {
    let yes: Vec<String> = vec!["newton".into(), "mcp".into(), "serve".into()];
    assert!(mcp::is_mcp_subcommand(&yes));

    let yes_with_flags: Vec<String> = vec![
        "newton".into(),
        "mcp".into(),
        "serve".into(),
        "--port".into(),
        "9999".into(),
    ];
    assert!(mcp::is_mcp_subcommand(&yes_with_flags));

    // Must return false for related-but-different forms.
    let no_serve: Vec<String> = vec!["newton".into(), "mcp".into()];
    assert!(!mcp::is_mcp_subcommand(&no_serve));

    let no_serve_with_mcp: Vec<String> = vec!["newton".into(), "serve".into(), "--with-mcp".into()];
    assert!(!mcp::is_mcp_subcommand(&no_serve_with_mcp));
}

#[test]
fn mcp_serve_subcommand_emits_structured_startup_log() {
    let port = pick_free_port();
    let bin = assert_cmd::cargo::cargo_bin("newton");
    let mut child = Command::new(bin)
        .arg("mcp")
        .arg("serve")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--path")
        .arg("/mcp")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn newton mcp serve");

    let stderr = child.stderr.take().expect("stderr pipe");
    let mut reader = BufReader::new(stderr);

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
    assert!(
        line.contains(&format!("\"tool_count\":{}", MCP_EXPOSED_COMMAND_IDS.len())),
        "line={}",
        line
    );
}

#[test]
fn mcp_serve_subcommand_tool_count_matches_exposed_ids() {
    assert_eq!(mcp::tool_count(), MCP_EXPOSED_COMMAND_IDS.len());
}

#[test]
fn mcp_serve_subcommand_port_conflict_emits_newton_mcp_001() {
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
