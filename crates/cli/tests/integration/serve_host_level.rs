//! Host-level endpoint tests: /api 308 redirect, /healthz, /readyz.
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn pick_free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().unwrap().port()
}

fn start_serve(port: u16) -> (std::process::Child, tempfile::TempDir) {
    start_serve_with(port, &[])
}

fn start_serve_with(port: u16, extra: &[&str]) -> (std::process::Child, tempfile::TempDir) {
    let dir = tempdir().expect("tempdir");
    let bin = assert_cmd::cargo::cargo_bin("newton");
    let mut args = vec!["serve", "--host", "127.0.0.1", "--port"];
    let port_s = port.to_string();
    args.push(&port_s);
    args.extend_from_slice(extra);
    let child = Command::new(bin)
        .current_dir(dir.path())
        .args(&args)
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
    // The deprecated `/health` API endpoint is gone (replaced by `/healthz`).
    // Run with --no-web so the SPA catch-all (which serves the UI for every
    // non-API path by default) doesn't mask the API-surface assertion.
    let port = pick_free_port();
    let (mut child, _dir) = start_serve_with(port, &["--no-web"]);

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

#[test]
fn embedded_web_ui_serves_spa_deeplinks_by_default() {
    // `newton serve` (no flags) serves the embedded UI at every non-API path,
    // including SPA deep links, with a clean 200 (not the prior ServeDir 404).
    let port = pick_free_port();
    let (mut child, _dir) = start_serve(port);

    if !wait_ready(port) {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not become ready within 30s");
    }

    let client = make_no_redirect_client();
    let mut results = Vec::new();
    for path in ["/", "/optimize", "/findings"] {
        let resp = client
            .get(format!("http://127.0.0.1:{}{}", port, path))
            .header("Accept-Encoding", "gzip")
            .send()
            .expect("ui request");
        let status = resp.status().as_u16();
        let ctype = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        results.push((path, status, ctype));
    }

    // healthz must still win over the UI fallback.
    let healthz = client
        .get(format!("http://127.0.0.1:{}/healthz", port))
        .send()
        .expect("healthz request")
        .status()
        .as_u16();

    let _ = child.kill();
    let _ = child.wait();

    for (path, status, ctype) in results {
        assert_eq!(status, 200, "{path} should serve the SPA with 200");
        assert!(
            ctype.starts_with("text/html"),
            "{path} should be text/html, got {ctype}"
        );
    }
    assert_eq!(healthz, 200, "/healthz must still be handled by the API");
}

#[test]
fn serve_prints_startup_banner_with_urls() {
    use std::io::Read;
    // `newton serve` must print a human-readable banner: its `info!` startup logs
    // are silenced in the serve console context and cli-framework prints nothing,
    // so without the banner the process looks like it hangs.
    let port = pick_free_port();
    let dir = tempdir().unwrap();
    let errpath = dir.path().join("stderr.log");
    let errfile = std::fs::File::create(&errpath).unwrap();
    let bin = assert_cmd::cargo::cargo_bin("newton");
    let mut child = Command::new(bin)
        .current_dir(dir.path())
        .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::from(errfile))
        .spawn()
        .expect("spawn newton serve");

    let ready = wait_ready(port);
    // The banner is flushed just before the listener binds; give it a beat.
    std::thread::sleep(Duration::from_millis(250));
    let _ = child.kill();
    let _ = child.wait();
    assert!(ready, "server did not become ready");

    let mut stderr = String::new();
    std::fs::File::open(&errpath)
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();

    assert!(
        stderr.contains("Newton serving on"),
        "startup banner missing; stderr=\n{stderr}"
    );
    assert!(
        stderr.contains(&format!("http://127.0.0.1:{port}/")),
        "web UI URL missing from banner; stderr=\n{stderr}"
    );
    assert!(
        stderr.contains("/api/v1/"),
        "REST API URL missing from banner; stderr=\n{stderr}"
    );
}
