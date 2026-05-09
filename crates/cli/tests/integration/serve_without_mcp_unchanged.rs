//! Issue #294: with `--with-mcp` absent, `newton serve` MUST behave as before:
//! no `mcp_serve_started` log line, no MCP route, `/health` still works.
//!
//! Note: `ExecutionContext::Server` disables tracing to stderr (`ConsoleOutput::None`),
//! so we cannot wait for "Newton API server listening" on stderr. Readiness is asserted
//! via `GET /health` polling instead.
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Wall-clock cap for the entire test (spawn, readiness, HTTP, cleanup).
const TEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Poll `/health` until success or this budget elapses (leave room for POST + cleanup under `TEST_TIMEOUT`).
const READINESS_POLL_BUDGET: Duration = Duration::from_secs(20);
/// Interval between readiness polls.
const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(150);
/// Short client timeouts while probing readiness (many iterations must fit inside `TEST_TIMEOUT`).
const READINESS_HTTP_CONNECT_TIMEOUT: Duration = Duration::from_millis(400);
const READINESS_HTTP_TOTAL_TIMEOUT: Duration = Duration::from_millis(800);
/// Max wait per stderr line while scanning for `mcp_serve_started` (stderr may be quiet).
const STDERR_LINE_WAIT: Duration = Duration::from_secs(2);
/// Per-request HTTP bound for assertions after the server is ready.
const HTTP_TIMEOUT: Duration = Duration::from_secs(6);

fn pick_free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().unwrap().port()
}

#[tokio::test]
async fn serve_default_does_not_emit_mcp_log_and_mcp_route_absent() {
    let outcome = tokio::time::timeout(TEST_TIMEOUT, run_serve_without_mcp_test()).await;

    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(e)) => panic!("{e}"),
        Err(_) => panic!(
            "serve_default_does_not_emit_mcp_log_and_mcp_route_absent exceeded {:?}",
            TEST_TIMEOUT
        ),
    }
}

async fn run_serve_without_mcp_test() -> Result<(), String> {
    let dir = tempdir().map_err(|e| format!("tempdir: {e}"))?;
    let port = pick_free_port();
    let bin = assert_cmd::cargo::cargo_bin("newton");

    let saw_mcp_event = Arc::new(AtomicBool::new(false));

    let mut child = Command::new(&bin)
        .current_dir(dir.path())
        .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn newton serve: {e}"))?;

    let stderr = child.stderr.take().ok_or("stderr not piped")?;
    let saw_err = Arc::clone(&saw_mcp_event);
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match tokio::time::timeout(STDERR_LINE_WAIT, reader.read_line(&mut line)).await {
                Err(_) => continue,
                Ok(Err(_)) => break,
                Ok(Ok(0)) => break,
                Ok(Ok(_)) => {
                    if line.contains("\"event\":\"mcp_serve_started\"")
                        || line.contains("mcp_serve_started")
                    {
                        saw_err.store(true, Ordering::SeqCst);
                    }
                }
            }
        }
    });

    let probe = reqwest::Client::builder()
        .connect_timeout(READINESS_HTTP_CONNECT_TIMEOUT)
        .timeout(READINESS_HTTP_TOTAL_TIMEOUT)
        .build()
        .map_err(|e| format!("reqwest probe client: {e}"))?;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| format!("reqwest client: {e}"))?;

    let health_url = format!("http://127.0.0.1:{}/health", port);
    let ready_deadline = tokio::time::Instant::now() + READINESS_POLL_BUDGET;
    let mut ready = false;
    while tokio::time::Instant::now() < ready_deadline {
        match probe.get(&health_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                ready = true;
                break;
            }
            _ => tokio::time::sleep(READINESS_POLL_INTERVAL).await,
        }
    }

    if !ready {
        stderr_task.abort();
        let _ = stderr_task.await;
        let _ = child.kill();
        let _ = tokio::time::timeout(Duration::from_secs(10), child.wait()).await;
        return Err(format!(
            "server did not become ready (GET {health_url}) within {:?}",
            READINESS_POLL_BUDGET
        ));
    }

    let mcp = tokio::time::timeout(
        HTTP_TIMEOUT + Duration::from_secs(1),
        client
            .post(format!("http://127.0.0.1:{}/mcp", port))
            .header("content-type", "application/json")
            .body("{}")
            .send(),
    )
    .await
    .map_err(|_| "/mcp request: timed out".to_string())?
    .map_err(|e| format!("/mcp request: {e}"))?;

    stderr_task.abort();
    let _ = stderr_task.await;

    let _ = child.kill();
    let _ = tokio::time::timeout(Duration::from_secs(10), child.wait()).await;

    assert!(
        !saw_mcp_event.load(Ordering::SeqCst),
        "mcp_serve_started was emitted without --with-mcp"
    );

    if mcp.status() != reqwest::StatusCode::NOT_FOUND {
        return Err(format!(
            "/mcp returned {} - should be 404 when --with-mcp is absent",
            mcp.status()
        ));
    }

    Ok(())
}
