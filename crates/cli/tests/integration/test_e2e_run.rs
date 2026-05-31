#[path = "../support/mod.rs"]
mod support;

use support::{fixture_path, newton, TempWorkspace};

#[test]
fn integ_run_workspace_creates_state() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/minimal_smoke.yaml");

    let out = newton()
        .args([
            "workflow",
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
        ])
        .output()
        .expect("newton workflow run should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "newton workflow run should succeed; stdout={stdout}, stderr={stderr}"
    );

    let workflows_dir = ws.path().join(".newton/state/workflows");
    assert!(
        workflows_dir.exists(),
        "run should create .newton/state/workflows/"
    );

    let has_run = std::fs::read_dir(&workflows_dir)
        .map(|mut entries| entries.any(|e| e.is_ok()))
        .unwrap_or(false);
    assert!(
        has_run,
        "run should create at least one run directory under workflows/"
    );
}

#[test]
fn integ_run_trigger_payload() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/minimal_smoke.yaml");

    let out = newton()
        .args([
            "workflow",
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
            "--trigger",
            "env=test",
        ])
        .output()
        .expect("newton workflow run --trigger should execute");

    assert!(
        out.status.success(),
        "newton workflow run --trigger should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn integ_run_multi_trigger_payload() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/minimal_smoke.yaml");

    let out = newton()
        .args([
            "workflow",
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
            "--trigger",
            "board_item_id=PVTI_test",
            "--trigger",
            "skip_idea_to_draft=true",
        ])
        .output()
        .expect("newton workflow run --trigger (multiple) should execute");

    assert!(
        out.status.success(),
        "newton workflow run with multiple --trigger flags should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn integ_run_deprecated_notice() {
    let wf = fixture_path("workflows/minimal_smoke.yaml");

    let out = newton()
        .args(["run", &wf.to_string_lossy()])
        .output()
        .expect("newton run (deprecated) should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "newton run (deprecated) should succeed; stdout={stdout}, stderr={stderr}"
    );
    assert!(
        stderr.contains("[newton] DEPRECATED"),
        "newton run should emit deprecation notice to stderr; stderr={stderr}"
    );
}
