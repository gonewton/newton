//! Issue #351: When both `--with-mcp` and `--with-embedded-ailoop` are active,
//! both `mcp_serve_started` and `ailoop_serve_started` JSON lines appear on stderr,
//! and non-colliding paths are accepted.
//!
//! NOTE: Full verification that ailoop routes are accessible (criterion 16 §3)
//! is pending the upstream Axum 0.8 upgrade (goailoop/ailoop#59). The test
//! verifying ailoop health under the base path is marked `#[ignore]`.
#[path = "../support/mod.rs"]
mod support;

use newton_cli::cli::mcp;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use tempfile::{tempdir, TempDir};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// A running `newton serve --with-mcp --with-embedded-ailoop` process.
///
/// Stderr is drained on a background thread so the child can never block on a
/// full pipe, and every line is kept so a failed wait can show what the server
/// actually printed. The child is killed on drop.
struct Serve {
    child: Child,
    port: u16,
    lines: Receiver<String>,
    seen: Vec<String>,
    _dir: TempDir,
}

impl Serve {
    fn start() -> Self {
        let dir = tempdir().expect("tempdir");
        let port = support::reserve_port();
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
                "--with-embedded-ailoop",
                "--ailoop-base-path",
                "/ailoop",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn newton serve --with-mcp --with-embedded-ailoop");

        let stderr = child.stderr.take().expect("stderr pipe");
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });

        Serve {
            child,
            port,
            lines,
            seen: Vec::new(),
            _dir: dir,
        }
    }

    /// Wait for the stderr JSON line carrying `"event":"<event>"` and return it.
    /// Panics with the collected stderr if the server exits or the timeout
    /// passes first.
    fn wait_for_event(&mut self, event: &str) -> String {
        let needle = format!("\"event\":\"{event}\"");
        if let Some(line) = self.seen.iter().find(|l| l.contains(&needle)) {
            return line.clone();
        }
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(remaining) {
                Ok(line) => {
                    let hit = line.contains(&needle);
                    self.seen.push(line.clone());
                    if hit {
                        return line;
                    }
                }
                Err(RecvTimeoutError::Timeout) => panic!(
                    "{event} not seen within {STARTUP_TIMEOUT:?}; stderr so far:\n{}",
                    self.seen.join("\n")
                ),
                Err(RecvTimeoutError::Disconnected) => panic!(
                    "newton serve exited before {event} (status {:?}); stderr:\n{}",
                    self.child.try_wait(),
                    self.seen.join("\n")
                ),
            }
        }
    }
}

impl Drop for Serve {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Criterion 16: both `mcp_serve_started` and `ailoop_serve_started` appear on
/// stderr when both flags are active.
#[test]
fn both_serve_started_events_emitted() {
    let mut serve = Serve::start();
    serve.wait_for_event("mcp_serve_started");
    serve.wait_for_event("ailoop_serve_started");
}

/// Criterion 15: non-colliding `--mcp-path /mcp` and `--ailoop-base-path /ailoop`
/// do not produce validation errors; the server starts successfully.
#[test]
fn non_colliding_mcp_and_ailoop_paths_start_successfully() {
    let mut serve = Serve::start();
    serve.wait_for_event("ailoop_serve_started");
}

/// Verifies `mcp_serve_started` event has expected fields alongside ailoop.
#[test]
fn mcp_serve_started_has_correct_fields_when_ailoop_also_active() {
    let mut serve = Serve::start();
    let mcp = serve.wait_for_event("mcp_serve_started");
    let ailoop = serve.wait_for_event("ailoop_serve_started");

    let expected_count = mcp::tool_count();
    assert!(
        mcp.contains(&format!("\"tool_count\":{expected_count}")),
        "mcp line={mcp}"
    );
    assert!(mcp.contains("\"mcp_path\":\"/mcp\""), "mcp line={mcp}");
    let bind_address = format!("\"bind_address\":\"127.0.0.1:{}\"", serve.port);
    assert!(mcp.contains(&bind_address), "mcp line={mcp}");

    assert!(
        ailoop.contains("\"ailoop_base_path\":\"/ailoop\""),
        "ailoop line={ailoop}"
    );
    assert!(
        ailoop.contains("\"ailoop_enabled\":true"),
        "ailoop line={ailoop}"
    );
    assert!(ailoop.contains(&bind_address), "ailoop line={ailoop}");
}

/// Full multi-surface test: `/health`, MCP, and ailoop routes all reachable.
///
/// No readiness polling: `newton serve` binds its listener before it emits
/// `ailoop_serve_started`, so once that line is seen the port is accepting
/// connections.
#[test]
fn all_surfaces_respond_when_both_flags_active() {
    let mut serve = Serve::start();
    serve.wait_for_event("ailoop_serve_started");
    let port = serve.port;

    let result = (|| -> Result<(), String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        rt.block_on(async {
            let client = reqwest::Client::new();

            let health = client
                .get(format!("http://127.0.0.1:{}/healthz", port))
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

    result.expect("all surfaces (/health, /mcp, /ailoop health) reachable");
}
