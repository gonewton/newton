//! Issue #294: `serve --with-mcp` mounts the MCP HTTP transport on the same
//! listener as the Newton REST API. We verify three things end-to-end:
//!
//! 1. A single structured `mcp_serve_started` JSON line is emitted on stderr
//!    after a successful bind, with `bind_address`, `mcp_path`, and a
//!    `tool_count` matching `cli::mcp::tool_count()` (Goal 4, criteria 3, 4).
//! 2. `GET /health` still works on the same listener (criterion 12).
//! 3. `POST <mcp-path>` reaches the MCP transport (any non-404 response).
//!
//! Like `mcp_on_starts_and_logs.rs`, the listener keeps running until killed.
use newton_cli::cli::mcp;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn pick_free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().unwrap().port()
}

#[test]
fn serve_with_mcp_emits_log_and_serves_both_surfaces() {
    let dir = tempdir().expect("tempdir");
    let port = pick_free_port();
    let bin = assert_cmd::cargo::cargo_bin("newton");
    let mut child = Command::new(bin)
        .current_dir(dir.path())
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--with-mcp",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn newton serve --with-mcp");

    let stderr = child.stderr.take().expect("stderr pipe");
    let mut reader = BufReader::new(stderr);

    // Wait for the structured startup line (cap at 30s — backend init does
    // SQLite migrations on first run).
    let deadline = Instant::now() + Duration::from_secs(30);
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

    let line = match found {
        Some(l) => l,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("structured mcp_serve_started log line not observed within 30s");
        }
    };

    // Validate fields.
    assert!(line.contains("\"mcp_enabled\":true"), "line={line}");
    assert!(
        line.contains(&format!("\"bind_address\":\"127.0.0.1:{}\"", port)),
        "line={line}"
    );
    assert!(line.contains("\"mcp_path\":\"/mcp\""), "line={line}");
    let expected_count = mcp::tool_count();
    assert!(
        line.contains(&format!("\"tool_count\":{}", expected_count)),
        "line={line}"
    );

    // Now hit both surfaces with blocking reqwest calls in a small runtime.
    let result = (|| -> Result<(), String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        rt.block_on(async {
            let client = reqwest::Client::new();
            let health_url = format!("http://127.0.0.1:{}/healthz", port);
            let mcp_url = format!("http://127.0.0.1:{}/mcp", port);

            // /health: well-defined Newton REST surface.
            let health = client
                .get(&health_url)
                .send()
                .await
                .map_err(|e| format!("/health request: {e}"))?;
            if !health.status().is_success() {
                return Err(format!("/health returned {}", health.status()));
            }

            // /mcp: any non-404 response means the route is mounted.
            // The MCP HTTP transport requires a session-init handshake, so a
            // bare POST will likely come back as a structured error rather
            // than 200; we just assert the route is reachable.
            let mcp = client
                .post(&mcp_url)
                .header("content-type", "application/json")
                .body("{}")
                .send()
                .await
                .map_err(|e| format!("/mcp request: {e}"))?;
            if mcp.status() == reqwest::StatusCode::NOT_FOUND {
                return Err("/mcp returned 404 — route not mounted".to_string());
            }
            Ok(())
        })
    })();

    let _ = child.kill();
    let _ = child.wait();
    result.expect("REST and MCP surfaces both reachable");
}
