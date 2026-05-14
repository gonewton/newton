//! Issue #351: `serve --with-embedded-ailoop` merges the ailoop HTTP/WebSocket
//! router onto the same Axum listener as Newton REST API, emits a structured
//! `ailoop_serve_started` JSON event on stderr, and serves both Newton REST
//! (`/health`) and ailoop health (`<base_path>/api/v1/health`) on the same port.
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn pick_free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().unwrap().port()
}

/// Starts `newton serve --with-embedded-ailoop` and waits for the structured
/// `ailoop_serve_started` JSON line on stderr. Returns the child process so
/// callers can clean it up.
fn start_embedded_ailoop_server(port: u16, base_path: &str) -> (std::process::Child, String) {
    let dir = tempdir().expect("tempdir");
    let bin = assert_cmd::cargo::cargo_bin("newton");
    let mut child = Command::new(bin)
        .current_dir(dir.path())
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--with-embedded-ailoop",
            "--ailoop-base-path",
            base_path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn newton serve --with-embedded-ailoop");

    let stderr = child.stderr.take().expect("stderr pipe");
    let mut reader = BufReader::new(stderr);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut found: Option<String> = None;
    while Instant::now() < deadline {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.contains("\"event\":\"ailoop_serve_started\"") {
                    found = Some(line);
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let startup_line = match found {
        Some(l) => l,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("structured ailoop_serve_started log line not observed within 30s");
        }
    };
    (child, startup_line)
}

/// Criteria 4 and 5: `ailoop_serve_started` JSON line is emitted with correct fields.
///
/// This test does NOT require the ailoop router to be merged — it only checks
/// the structured log event emitted to stderr.
#[test]
fn ailoop_serve_started_event_has_correct_fields() {
    let port = pick_free_port();
    let base_path = "/ailoop";
    let (mut child, line) = start_embedded_ailoop_server(port, base_path);

    let _ = child.kill();
    let _ = child.wait();

    assert!(line.contains("\"ailoop_enabled\":true"), "line={line}");
    assert!(
        line.contains(&format!("\"bind_address\":\"127.0.0.1:{}\"", port)),
        "line={line}"
    );
    assert!(
        line.contains(&format!("\"ailoop_base_path\":\"{base_path}\"")),
        "line={line}"
    );
}

/// Criterion 3: Newton REST `/health` endpoint is reachable on the same port.
#[test]
fn newton_health_responds_on_same_port() {
    let port = pick_free_port();
    let (mut child, _line) = start_embedded_ailoop_server(port, "/ailoop");

    let result = (|| -> Result<(), String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        rt.block_on(async {
            let client = reqwest::Client::new();
            let health_url = format!("http://127.0.0.1:{}/health", port);
            let resp = client
                .get(&health_url)
                .send()
                .await
                .map_err(|e| format!("/health request: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("/health returned {}", resp.status()));
            }
            Ok(())
        })
    })();

    let _ = child.kill();
    let _ = child.wait();
    result.expect("Newton REST /health reachable on same port as embedded ailoop");
}

/// Criteria 1 and 2: ailoop health endpoint responds under the base path.
#[test]
fn ailoop_health_endpoint_responds_under_base_path() {
    let port = pick_free_port();
    let base_path = "/ailoop";
    let (mut child, _line) = start_embedded_ailoop_server(port, base_path);

    let result = (|| -> Result<(), String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        rt.block_on(async {
            let client = reqwest::Client::new();
            let ailoop_health_url = format!("http://127.0.0.1:{}{}/api/v1/health", port, base_path);
            let resp = client
                .get(&ailoop_health_url)
                .send()
                .await
                .map_err(|e| format!("ailoop health request: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!(
                    "ailoop health returned {} (expected 200)",
                    resp.status()
                ));
            }
            Ok(())
        })
    })();

    let _ = child.kill();
    let _ = child.wait();
    result.expect("ailoop health endpoint reachable under base path");
}

/// §4.7 CORS verification: OPTIONS preflight on an ailoop route returns 2xx.
#[test]
fn cors_options_preflight_on_ailoop_route_returns_2xx() {
    let port = pick_free_port();
    let base_path = "/ailoop";
    let (mut child, _line) = start_embedded_ailoop_server(port, base_path);

    let result = (|| -> Result<(), String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        rt.block_on(async {
            let client = reqwest::Client::new();
            let url = format!("http://127.0.0.1:{}{}/api/v1/health", port, base_path);
            let resp = client
                .request(reqwest::Method::OPTIONS, &url)
                .header("Origin", "http://example.com")
                .header("Access-Control-Request-Method", "GET")
                .send()
                .await
                .map_err(|e| format!("OPTIONS preflight: {e}"))?;
            let status = resp.status().as_u16();
            if !(200..300).contains(&status) {
                return Err(format!(
                    "OPTIONS preflight on {url} returned {status}, expected 2xx"
                ));
            }
            Ok(())
        })
    })();

    let _ = child.kill();
    let _ = child.wait();
    result.expect("OPTIONS preflight on ailoop route returns 2xx");
}
