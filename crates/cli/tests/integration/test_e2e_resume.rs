#[path = "../support/mod.rs"]
mod support;

use std::fs;
use std::path::Path;
use support::{fixture_path, newton, TempWorkspace};

const RESUME_RUN_ID: &str = "11111111-2222-3333-4444-555555555555";

/// Seeds `<state_root>/workflows/<run_id>/{execution,checkpoint}.json` with a
/// not-yet-completed run for `workflow_fixture` (a path under
/// `tests/fixtures/`), whose entry task(s) are `ready_queue`. `state_root` is
/// the resolved state root (`<workspace>/.newton/state` by default, or an
/// arbitrary `--state-dir` override) — MUST match `state_checkpoints_dir`'s
/// layout (`<state_root>/workflows/<run_id>/...`) so `resume` finds it.
fn seed_resume_run_at(
    state_root: &Path,
    run_id: &str,
    workflow_fixture: &str,
    ready_queue: &[&str],
) {
    let run_dir = state_root.join("workflows").join(run_id);
    fs::create_dir_all(&run_dir).unwrap();

    let wf_abs = fixture_path(workflow_fixture);
    let execution = serde_json::json!({
        "format_version": "1",
        "execution_id": run_id,
        "workflow_file": wf_abs.to_string_lossy(),
        "workflow_version": "2.0",
        "workflow_hash": "0000000000000000000000000000000000000000000000000000000000000000",
        "started_at": "2026-01-01T00:00:00Z",
        "completed_at": null,
        "status": "Running",
        "task_runs": [],
        "settings_effective": {
            "entry_task": ready_queue.first().copied().unwrap_or("noop"),
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
        "execution_id": run_id,
        "workflow_hash": "0000000000000000000000000000000000000000000000000000000000000000",
        "created_at": "2026-01-01T00:00:01Z",
        "ready_queue": ready_queue,
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

fn seed_resume_run(ws: &TempWorkspace) {
    seed_resume_run_at(
        &ws.path().join(".newton/state"),
        RESUME_RUN_ID,
        "workflows/minimal_smoke.yaml",
        &["noop"],
    );
}

#[test]
fn integ_resume_run_id() {
    let ws = TempWorkspace::new();
    seed_resume_run(&ws);

    let out = newton()
        .args([
            "workflow",
            "resume",
            "--run-id",
            RESUME_RUN_ID,
            "--workspace",
            &ws.path().to_string_lossy(),
            "--allow-workflow-change",
        ])
        .output()
        .expect("newton workflow resume should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "newton workflow resume should succeed; stdout={stdout}, stderr={stderr}"
    );
    assert!(
        stdout.contains(RESUME_RUN_ID),
        "resume output should contain run id; got: {stdout}"
    );
}

/// spec 074, P6 (gap 3 — no `--emit-completion-json` support): resuming with
/// the flag prints the same structured completion envelope `run` does, not
/// just the human-readable "Workflow resumed..." line.
#[test]
fn integ_resume_emit_completion_json_prints_envelope() {
    let ws = TempWorkspace::new();
    seed_resume_run(&ws);

    let out = newton()
        .args([
            "workflow",
            "resume",
            "--run-id",
            RESUME_RUN_ID,
            "--workspace",
            &ws.path().to_string_lossy(),
            "--allow-workflow-change",
            "--emit-completion-json",
        ])
        .output()
        .expect("newton workflow resume should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "resume --emit-completion-json should succeed; stdout={stdout}, stderr={stderr}"
    );
    let envelope: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be valid JSON: {e}; stdout={stdout}"));
    assert_eq!(
        envelope["status"], "success",
        "status must be 'success'; envelope={envelope}"
    );
    assert_eq!(
        envelope["execution_id"], RESUME_RUN_ID,
        "envelope execution_id must match the resumed run; envelope={envelope}"
    );
}

/// spec 074, P6 (gap 4 — `resume_workflow` hardcoded
/// `ExecutionOverrides::default()`): `--state-dir` on `resume` MUST relocate
/// the backend store the same way it does for `run` (prior art:
/// `test_state_dir_one_root.rs`). Before the fix, the resumed run's sink was
/// always `None` and the checkpoint root was always the workspace default,
/// regardless of `--state-dir` — a resumed grading workflow's writes would
/// never reach the override store.
#[test]
fn integ_resume_state_dir_uses_override_root() {
    let ws = TempWorkspace::new();
    let ws_path = ws.path().to_string_lossy().to_string();

    let override_dir = tempfile::tempdir().expect("override state dir");
    let override_path = override_dir.path().to_string_lossy().to_string();

    // Seed the checkpoint under the OVERRIDE state root, exactly where
    // `resolve_state_dir` + `state_checkpoints_dir` will look given
    // `--state-dir <override_path>`.
    seed_resume_run_at(
        override_dir.path(),
        RESUME_RUN_ID,
        "workflows/minimal_smoke.yaml",
        &["noop"],
    );

    let out = newton()
        .args([
            "workflow",
            "resume",
            "--run-id",
            RESUME_RUN_ID,
            "--workspace",
            &ws_path,
            "--state-dir",
            &override_path,
            "--allow-workflow-change",
        ])
        .output()
        .expect("newton workflow resume should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "resume --state-dir should succeed; stdout={stdout}, stderr={stderr}"
    );

    let override_db = override_dir.path().join("backend.sqlite");
    assert!(
        override_db.exists(),
        "expected backend.sqlite under the override state dir at {}",
        override_db.display()
    );

    let default_db = ws.path().join(".newton/state/backend.sqlite");
    assert!(
        !default_db.exists(),
        "workspace-default backend.sqlite must not exist — resume must have used ONLY the \
         override state dir, not split-brained against the workspace default: {}",
        default_db.display()
    );
}

/// spec 074, P6 (gap 4 — `--verbose` never propagated to `resume_workflow`):
/// resuming with `--verbose` prints the resumed task's captured stdout to the
/// terminal, exactly like `run --verbose` (prior art:
/// `test_run_input_file_verbose.rs::verbose_prints_captured_task_output_to_terminal`).
#[test]
fn integ_resume_verbose_prints_captured_output() {
    let ws = TempWorkspace::new();
    seed_resume_run_at(
        &ws.path().join(".newton/state"),
        RESUME_RUN_ID,
        "workflows/verbose_marker.yaml",
        &["echo_marker"],
    );

    let out = newton()
        .args([
            "workflow",
            "resume",
            "--run-id",
            RESUME_RUN_ID,
            "--workspace",
            &ws.path().to_string_lossy(),
            "--allow-workflow-change",
            "--verbose",
        ])
        .output()
        .expect("newton workflow resume should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "resume --verbose should succeed; stdout={stdout}, stderr={stderr}"
    );
    assert!(
        stdout.contains("VERBOSE_MARKER_9f3a1c"),
        "expected the resumed task's captured stdout marker in --verbose output: {stdout}"
    );
    assert!(
        stdout.contains("echo_marker"),
        "expected a per-task header naming the resumed task id: {stdout}"
    );
}
