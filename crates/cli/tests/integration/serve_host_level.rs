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

#[test]
fn default_serve_binds_loopback_without_exposure_warning() {
    use std::io::Read;
    // Default `--host` (127.0.0.1) with no OIDC configured must start
    // unauthenticated with no exposure warning; the "Auth disabled" banner
    // line is the friction-free local-tool default.
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
        !stderr.contains("UNAUTHENTICATED"),
        "default loopback bind must not print the unauthenticated-exposure warning; stderr=\n{stderr}"
    );
    assert!(
        stderr.contains("Auth       disabled"),
        "default loopback bind should show the auth-disabled banner line; stderr=\n{stderr}"
    );
}

#[test]
fn non_loopback_host_without_oidc_refuses_to_start() {
    use std::io::Read;
    // `--host` itself is the explicit opt-in to non-loopback exposure (spec 074
    // PR-6 / B5) — no separate flag. Since audit finding C5, binding
    // non-loopback WITHOUT OIDC configured must fail closed: the process must
    // refuse to start (never bind the listener) and print an actionable error
    // naming the exact flags/env vars needed, rather than booting wide open
    // behind a warning banner.
    let port = pick_free_port();
    let dir = tempdir().unwrap();
    let errpath = dir.path().join("stderr.log");
    let errfile = std::fs::File::create(&errpath).unwrap();
    let bin = assert_cmd::cargo::cargo_bin("newton");
    let mut child = Command::new(bin)
        .current_dir(dir.path())
        .args(["serve", "--host", "0.0.0.0", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::from(errfile))
        .spawn()
        .expect("spawn newton serve");

    let ready = wait_ready(port);
    let status = child.wait().expect("wait for newton serve to exit");

    assert!(
        !ready,
        "non-loopback bind without OIDC must never become ready (must not bind the listener)"
    );
    assert!(
        !status.success(),
        "non-loopback bind without OIDC must exit non-zero, got {status:?}"
    );

    let mut stderr = String::new();
    std::fs::File::open(&errpath)
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();

    assert!(
        stderr.contains("NEWTON-SERVE-AUTH-001"),
        "expected the fail-closed error code in stderr; stderr=\n{stderr}"
    );
    assert!(
        stderr.contains("--oidc-issuer") && stderr.contains("--oidc-audience"),
        "error must name the flags needed to unblock; stderr=\n{stderr}"
    );
}

#[test]
fn non_loopback_host_with_oidc_starts_and_gates_the_api() {
    use std::io::Read;
    // With OIDC configured, a non-loopback bind is allowed to start (the only
    // way it's allowed to start), and the REST API is gated: an
    // unauthenticated request to a protected route gets 401 with a
    // `WWW-Authenticate: Bearer` challenge, while `/healthz` stays public.
    let port = pick_free_port();
    let dir = tempdir().unwrap();
    let errpath = dir.path().join("stderr.log");
    let errfile = std::fs::File::create(&errpath).unwrap();
    let bin = assert_cmd::cargo::cargo_bin("newton");
    let mut child = Command::new(bin)
        .current_dir(dir.path())
        .args([
            "serve",
            "--host",
            "0.0.0.0",
            "--port",
            &port.to_string(),
            "--oidc-issuer",
            "https://issuer.example.invalid/realms/newton",
            "--oidc-audience",
            "newton-api",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::from(errfile))
        .spawn()
        .expect("spawn newton serve");

    let ready = wait_ready(port);
    if !ready {
        let _ = child.kill();
        let _ = child.wait();
        let mut stderr = String::new();
        let _ = std::fs::File::open(&errpath).and_then(|mut f| f.read_to_string(&mut stderr));
        panic!("server did not become ready within 30s; stderr=\n{stderr}");
    }

    let client = make_no_redirect_client();

    // Public: /healthz is never wrapped by the auth layer.
    let healthz_status = client
        .get(format!("http://127.0.0.1:{}/healthz", port))
        .send()
        .expect("/healthz request")
        .status()
        .as_u16();

    // Gated: an unauthenticated request to the REST API must be rejected.
    let api_resp = client
        .get(format!("http://127.0.0.1:{}/api/v1/operators", port))
        .send()
        .expect("/api/v1/operators request");
    let api_status = api_resp.status().as_u16();
    let www_authenticate = api_resp
        .headers()
        .get("www-authenticate")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(healthz_status, 200, "/healthz must stay public");
    assert_eq!(
        api_status, 401,
        "unauthenticated REST API request must be rejected once OIDC is configured"
    );
    assert!(
        www_authenticate.to_lowercase().contains("bearer"),
        "401 must carry a WWW-Authenticate: Bearer challenge, got {www_authenticate:?}"
    );
}
