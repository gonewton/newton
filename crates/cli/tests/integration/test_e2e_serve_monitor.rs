use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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

    let mut child = Command::new(bin)
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

#[test]
#[ignore]
fn ext_monitor_help_runs() {
    let bin = assert_cmd::cargo::cargo_bin("newton");
    let out = Command::new(bin)
        .args(["monitor", "--help"])
        .output()
        .expect("monitor --help should run");
    assert!(
        out.status.success(),
        "monitor --help should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}
