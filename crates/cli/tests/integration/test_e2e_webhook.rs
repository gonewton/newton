use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn pick_free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().unwrap().port()
}

#[test]
#[ignore]
fn ext_webhook_serve_starts() {
    let port = pick_free_port();
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("workspace");
    std::fs::create_dir_all(ws.join(".newton/state/workflows")).unwrap();
    std::fs::create_dir_all(ws.join(".newton/state/artifacts")).unwrap();

    let wf_content = "version: \"2.0\"\nmode: \"workflow_graph\"\nmetadata:\n  name: \"webhook test\"\nworkflow:\n  settings:\n    entry_task: \"noop\"\n    max_time_seconds: 30\n    parallel_limit: 1\n    continue_on_error: false\n    max_task_iterations: 1\n    max_workflow_iterations: 5\n  tasks:\n    - id: \"noop\"\n      operator: \"NoOpOperator\"\n      terminal: success\n";
    let wf_path = ws.join("workflow.yaml");
    std::fs::write(&wf_path, wf_content).unwrap();

    let bin = assert_cmd::cargo::cargo_bin("newton");
    let mut child = Command::new(bin)
        .current_dir(&ws)
        .args([
            "webhook",
            "serve",
            "--workflow",
            &wf_path.to_string_lossy(),
            "--workspace",
            &ws.to_string_lossy(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NEWTON_WEBHOOK_PORT", port.to_string())
        .spawn()
        .expect("spawn newton webhook serve");

    let stderr = child.stderr.take().expect("stderr pipe");
    let mut reader = BufReader::new(stderr);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut started = false;

    while Instant::now() < deadline {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.contains("listening")
                    || line.contains("started")
                    || line.contains("webhook")
                        && (line.contains("serve") || line.contains("ready"))
                {
                    started = true;
                    break;
                }
            }
            Err(_) => break,
        }
    }

    if !started {
        std::thread::sleep(Duration::from_millis(500));
    }

    let _ = child.kill();
    let _ = child.wait();
}
