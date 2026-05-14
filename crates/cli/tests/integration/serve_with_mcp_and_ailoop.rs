//! Issue #351: When both `--with-mcp` and `--with-embedded-ailoop` are active,
//! both `mcp_serve_started` and `ailoop_serve_started` JSON lines appear on stderr,
//! and non-colliding paths are accepted.
//!
//! NOTE: Full verification that ailoop routes are accessible (criterion 16 §3)
//! is pending the upstream Axum 0.8 upgrade (goailoop/ailoop#59). The test
//! verifying ailoop health under the base path is marked `#[ignore]`.
use newton_cli::cli::mcp;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn pick_free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().unwrap().port()
}

/// Criterion 16: both `mcp_serve_started` and `ailoop_serve_started` appear on
/// stderr when both flags are active.
#[test]
fn both_serve_started_events_emitted() {
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
            "--mcp-path",
            "/mcp",
            "--with-embedded-ailoop",
            "--ailoop-base-path",
            "/ailoop",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn newton serve --with-mcp --with-embedded-ailoop");

    let stderr = child.stderr.take().expect("stderr pipe");
    let mut reader = BufReader::new(stderr);

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut mcp_found = false;
    let mut ailoop_found = false;

    while Instant::now() < deadline && !(mcp_found && ailoop_found) {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.contains("\"event\":\"mcp_serve_started\"") {
                    mcp_found = true;
                }
                if line.contains("\"event\":\"ailoop_serve_started\"") {
                    ailoop_found = true;
                }
            }
            Err(_) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        mcp_found,
        "expected mcp_serve_started JSON line on stderr within 30s"
    );
    assert!(
        ailoop_found,
        "expected ailoop_serve_started JSON line on stderr within 30s"
    );
}

/// Criterion 15: non-colliding `--mcp-path /mcp` and `--ailoop-base-path /ailoop`
/// do not produce validation errors; the server starts successfully.
#[test]
fn non_colliding_mcp_and_ailoop_paths_start_successfully() {
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
            "--mcp-path",
            "/mcp",
            "--with-embedded-ailoop",
            "--ailoop-base-path",
            "/ailoop",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn newton serve");

    let stderr = child.stderr.take().expect("stderr pipe");
    let mut reader = BufReader::new(stderr);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut started = false;
    while Instant::now() < deadline {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.contains("\"event\":\"ailoop_serve_started\"")
                    || line.contains("\"event\":\"mcp_serve_started\"")
                {
                    started = true;
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    assert!(started, "server did not start within 30s");
}

/// Verifies `mcp_serve_started` event has expected fields alongside ailoop.
#[test]
fn mcp_serve_started_has_correct_fields_when_ailoop_also_active() {
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
            "--mcp-path",
            "/mcp",
            "--with-embedded-ailoop",
            "--ailoop-base-path",
            "/ailoop",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn newton serve");

    let stderr = child.stderr.take().expect("stderr pipe");
    let mut reader = BufReader::new(stderr);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut mcp_line: Option<String> = None;
    let mut ailoop_line: Option<String> = None;

    while Instant::now() < deadline && (mcp_line.is_none() || ailoop_line.is_none()) {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.contains("\"event\":\"mcp_serve_started\"") && mcp_line.is_none() {
                    mcp_line = Some(line.clone());
                }
                if line.contains("\"event\":\"ailoop_serve_started\"") && ailoop_line.is_none() {
                    ailoop_line = Some(line.clone());
                }
            }
            Err(_) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    let mcp = mcp_line.expect("mcp_serve_started not found within 30s");
    let ailoop = ailoop_line.expect("ailoop_serve_started not found within 30s");

    let expected_count = mcp::tool_count();
    assert!(
        mcp.contains(&format!("\"tool_count\":{expected_count}")),
        "mcp line={mcp}"
    );
    assert!(mcp.contains("\"mcp_path\":\"/mcp\""), "mcp line={mcp}");

    assert!(
        ailoop.contains("\"ailoop_base_path\":\"/ailoop\""),
        "ailoop line={ailoop}"
    );
    assert!(
        ailoop.contains("\"ailoop_enabled\":true"),
        "ailoop line={ailoop}"
    );
}

/// Full multi-surface test: `/health`, MCP, and ailoop routes all reachable.
///
/// Requires the upstream ailoop-server Axum 0.8 upgrade (goailoop/ailoop#59)
/// for the ailoop health assertion. Ignored until Stage 2 lands.
#[test]
#[ignore = "pending goailoop/ailoop#59 upstream Axum 0.8 upgrade and router merge in commands.rs"]
fn all_surfaces_respond_when_both_flags_active() {
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
            "--mcp-path",
            "/mcp",
            "--with-embedded-ailoop",
            "--ailoop-base-path",
            "/ailoop",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn newton serve");

    let stderr = child.stderr.take().expect("stderr pipe");
    let mut reader = BufReader::new(stderr);
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let mut line = String::new();
        if matches!(reader.read_line(&mut line), Ok(n) if n > 0) {
            if line.contains("\"event\":\"ailoop_serve_started\"") {
                break;
            }
        } else {
            break;
        }
    }

    let result = (|| -> Result<(), String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        rt.block_on(async {
            let client = reqwest::Client::new();

            let health = client
                .get(format!("http://127.0.0.1:{}/health", port))
                .send()
                .await
                .map_err(|e| format!("/health: {e}"))?;
            if !health.status().is_success() {
                return Err(format!("/health returned {}", health.status()));
            }

            let mcp = client
                .post(format!("http://127.0.0.1:{}/mcp", port))
                .header("content-type", "application/json")
                .body("{}")
                .send()
                .await
                .map_err(|e| format!("/mcp: {e}"))?;
            if mcp.status() == reqwest::StatusCode::NOT_FOUND {
                return Err("/mcp returned 404".to_string());
            }

            let ailoop_health = client
                .get(format!("http://127.0.0.1:{}/ailoop/api/v1/health", port))
                .send()
                .await
                .map_err(|e| format!("/ailoop health: {e}"))?;
            if !ailoop_health.status().is_success() {
                return Err(format!(
                    "/ailoop/api/v1/health returned {}",
                    ailoop_health.status()
                ));
            }

            Ok(())
        })
    })();

    let _ = child.kill();
    let _ = child.wait();
    result.expect("all surfaces (/health, /mcp, /ailoop health) reachable");
}
