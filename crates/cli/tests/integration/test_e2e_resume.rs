#[path = "../support/mod.rs"]
mod support;

use std::fs;
use support::{fixture_path, newton, TempWorkspace};

const RESUME_RUN_ID: &str = "11111111-2222-3333-4444-555555555555";

fn seed_resume_run(ws: &TempWorkspace) {
    let run_dir = ws
        .path()
        .join(".newton/state/workflows")
        .join(RESUME_RUN_ID);
    fs::create_dir_all(&run_dir).unwrap();

    let wf_abs = fixture_path("workflows/minimal_smoke.yaml");
    let execution = serde_json::json!({
        "format_version": "1",
        "execution_id": RESUME_RUN_ID,
        "workflow_file": wf_abs.to_string_lossy(),
        "workflow_version": "2.0",
        "workflow_hash": "0000000000000000000000000000000000000000000000000000000000000000",
        "started_at": "2026-01-01T00:00:00Z",
        "completed_at": null,
        "status": "Running",
        "task_runs": [],
        "settings_effective": {
            "entry_task": "noop",
            "max_time_seconds": 30,
            "parallel_limit": 1,
            "continue_on_error": false,
            "max_task_iterations": 1,
            "max_workflow_iterations": 5
        },
        "trigger_payload": {},
        "nesting_depth": 0,
    });
    fs::write(
        run_dir.join("execution.json"),
        serde_json::to_string_pretty(&execution).unwrap(),
    )
    .unwrap();

    let checkpoint = serde_json::json!({
        "format_version": "1",
        "execution_id": RESUME_RUN_ID,
        "workflow_hash": "0000000000000000000000000000000000000000000000000000000000000000",
        "created_at": "2026-01-01T00:00:01Z",
        "ready_queue": ["noop"],
        "context": {},
        "trigger_payload": {},
        "task_iterations": {},
        "total_iterations": 0,
        "completed": {},
        "version": 1,
    });
    fs::write(
        run_dir.join("checkpoint.json"),
        serde_json::to_string_pretty(&checkpoint).unwrap(),
    )
    .unwrap();
}

#[test]
fn integ_resume_run_id() {
    let ws = TempWorkspace::new();
    seed_resume_run(&ws);

    let out = newton()
        .args([
            "resume",
            "--run-id",
            RESUME_RUN_ID,
            "--workspace",
            &ws.path().to_string_lossy(),
            "--allow-workflow-change",
        ])
        .output()
        .expect("newton resume should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "newton resume should succeed; stdout={stdout}, stderr={stderr}"
    );
    assert!(
        stdout.contains(RESUME_RUN_ID),
        "resume output should contain run id; got: {stdout}"
    );
}
