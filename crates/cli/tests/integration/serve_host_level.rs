//! Host-level endpoint tests: /api 308 redirect, /healthz, /readyz.
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn pick_free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().unwrap().port()
}

fn start_serve(port: u16) -> (std::process::Child, tempfile::TempDir) {
    let dir = tempdir().expect("tempdir");
    let bin = assert_cmd::cargo::cargo_bin("newton");
    let child = Command::new(bin)
        .current_dir(dir.path())
        .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn newton serve");
    (child, dir)
}

fn make_no_redirect_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_millis(300))
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client build")
}

fn wait_ready(port: u16) -> bool {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_millis(300))
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client");
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Ok(resp) = client
            .get(format!("http://127.0.0.1:{}/healthz", port))
            .send()
        {
            if resp.status().is_success() {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

#[test]
fn healthz_returns_200_with_version() {
    let port = pick_free_port();
    let (mut child, _dir) = start_serve(port);

    if !wait_ready(port) {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not become ready within 30s");
    }

    let client = make_no_redirect_client();
    let resp = client
        .get(format!("http://127.0.0.1:{}/healthz", port))
        .send()
        .expect("/healthz request");

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        resp.status().is_success(),
        "/healthz returned {}",
        resp.status()
    );
    let body: serde_json::Value = resp.json().expect("JSON body");
    assert_eq!(body["status"], "ok", "status field: {body}");
    assert!(body["version"].is_string(), "version field: {body}");
}

#[test]
fn readyz_returns_200() {
    let port = pick_free_port();
    let (mut child, _dir) = start_serve(port);

    if !wait_ready(port) {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not become ready within 30s");
    }

    let client = make_no_redirect_client();
    let resp = client
        .get(format!("http://127.0.0.1:{}/readyz", port))
        .send()
        .expect("/readyz request");

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        resp.status().is_success(),
        "/readyz returned {}",
        resp.status()
    );
}

#[test]
fn api_root_redirects_to_v1() {
    let port = pick_free_port();
    let (mut child, _dir) = start_serve(port);

    if !wait_ready(port) {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not become ready within 30s");
    }

    let client = make_no_redirect_client();
    let resp = client
        .get(format!("http://127.0.0.1:{}/api", port))
        .send()
        .expect("/api request");

    let status = resp.status().as_u16();
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        status == 308 || (300..400).contains(&status),
        "/api must redirect, got {status}"
    );
}

#[test]
fn health_old_path_returns_404() {
    let port = pick_free_port();
    let (mut child, _dir) = start_serve(port);

    if !wait_ready(port) {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not become ready within 30s");
    }

    let client = make_no_redirect_client();
    let resp = client
        .get(format!("http://127.0.0.1:{}/health", port))
        .send()
        .expect("/health request");

    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(
        resp.status().as_u16(),
        404,
        "/health must return 404 after migration"
    );
}
