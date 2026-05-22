#[path = "../support/mod.rs"]
mod support;

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command as StdCommand, Stdio};
use std::time::{Duration, Instant};
use support::newton;

#[test]
fn integ_health_command() {
    let out = newton()
        .args(["health"])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("OK") || stdout.contains("ok"),
        "health should report OK; got: {stdout}"
    );
}

#[test]
fn integ_doctor_command() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".newton")).unwrap();
    let out = newton()
        .args(["doctor", "--workspace", &dir.path().to_string_lossy()])
        .output()
        .expect("newton doctor should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("OK") || stdout.contains("SKIP") || stdout.contains("FAIL"),
        "doctor should produce probe output; got: {stdout}"
    );
}

#[test]
fn integ_config_show() {
    let out = newton()
        .args(["config", "show"])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("config show must emit valid JSON");
    assert!(
        parsed.get("newton_version").is_some(),
        "config show JSON should contain newton_version; got: {stdout}"
    );
}

#[test]
fn integ_completion_bash() {
    let out = newton()
        .args(["completion", "bash"])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.is_empty(), "completion bash should produce output");
    let first_line = stdout.lines().next().unwrap_or("");
    assert!(
        first_line.starts_with("_newton()"),
        "completion bash first line should start with '_newton()'; got: {first_line}"
    );
}

fn pick_free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().unwrap().port()
}

fn kill_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
#[ignore]
fn ext_serve_ephemeral_port_health() {
    let port = pick_free_port();
    let dir = tempfile::tempdir().unwrap();
    let bin = assert_cmd::cargo::cargo_bin("newton");

    let mut child = StdCommand::new(bin)
        .current_dir(dir.path())
        .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn newton serve");

    let stderr = child.stderr.take().expect("stderr pipe");
    let mut reader = BufReader::new(stderr);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut ready = false;

    while Instant::now() < deadline {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.contains("listening") || line.contains("started") || line.contains("bound")
                {
                    ready = true;
                    break;
                }
            }
            Err(_) => break,
        }
    }

    if !ready {
        std::thread::sleep(Duration::from_secs(2));
    }

    let result = (|| -> Result<(), String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        rt.block_on(async {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .map_err(|e| format!("client: {e}"))?;
            let url = format!("http://127.0.0.1:{}/health", port);
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("health request: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!("/health returned {}", resp.status()));
            }
            Ok(())
        })
    })();

    kill_child(&mut child);
    result.expect("serve ephemeral port health check");
}
