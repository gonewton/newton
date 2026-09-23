//! `newton serve` must keep serving when its stderr is closed.
//!
//! The startup events and banner are written to stderr after the listener is
//! bound. `eprintln!` panics on a write error, so a closed stderr (a supervisor
//! that stops reading, `newton serve 2>&1 | head`, or a test harness that drops
//! its pipe after the `*_serve_started` line) used to take the server down
//! right after it had bound its port: the panic unwound out of `serve`,
//! dropping the listener, and the process then either exited 101 or lingered
//! while the tokio runtime waited on its blocking threads.
//!
//! The read end of the stderr pipe is closed before the child starts, so every
//! stderr write fails with EPIPE: the test does not depend on timing.
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::tempdir;

/// Pick a port for `newton serve --port` (which rejects `0`). Candidates come
/// from 20000..30000, below Linux's ephemeral range, so the kernel doesn't
/// hand the port to an outbound connection before `newton` binds it (see
/// aroff/cli-framework#153).
fn reserve_port() -> u16 {
    const BASE: u32 = 20_000;
    const SPAN: u32 = 10_000;
    let seed = std::process::id();
    for offset in 0..SPAN {
        let candidate = (BASE + seed.wrapping_add(offset) % SPAN) as u16;
        if std::net::TcpListener::bind(("127.0.0.1", candidate)).is_ok() {
            return candidate;
        }
    }
    panic!("no free TCP port in {}..{}", BASE, BASE + SPAN);
}

#[test]
fn serve_keeps_serving_when_stderr_is_closed() {
    let dir = tempdir().expect("tempdir");
    let port = reserve_port();

    let (reader, writer) = std::io::pipe().expect("pipe");
    drop(reader);

    let mut child = Command::new(assert_cmd::cargo::cargo_bin("newton"))
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
        .stderr(Stdio::from(writer))
        .spawn()
        .expect("spawn newton serve");

    // With stderr closed there is no startup event to wait for, so readiness
    // is observed the way an orchestrator would: `/healthz` answering 200. A
    // server that crashed on its startup output never gets there, and
    // `try_wait` below reports its exit status.
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_millis(300))
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client");
    let url = format!("http://127.0.0.1:{port}/healthz");
    let deadline = Instant::now() + Duration::from_secs(30);
    let outcome = loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            break Err(format!(
                "newton serve exited with a closed stderr: {status}"
            ));
        }
        if let Ok(resp) = client.get(&url).send() {
            if resp.status().is_success() {
                break Ok(());
            }
        }
        if Instant::now() >= deadline {
            break Err("/healthz not answering 200 within 30s".to_string());
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    let still_running = child.try_wait().expect("try_wait").is_none();
    let _ = child.kill();
    let _ = child.wait();
    outcome.expect("server keeps serving with stderr closed");
    assert!(still_running, "server exited after answering /healthz");
}
