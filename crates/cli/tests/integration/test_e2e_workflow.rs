#[path = "../support/mod.rs"]
mod support;

use support::{fixture_path, newton};

#[test]
fn integ_workflow_validate_ok() {
    let wf = fixture_path("workflows/minimal_smoke.yaml");
    let out = newton()
        .args(["workflow", "validate", &wf.to_string_lossy()])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.to_lowercase().contains("valid"),
        "expected 'valid' in output: {stdout}"
    );
}

#[test]
fn integ_workflow_lint_json() {
    let wf = fixture_path("workflows/minimal_smoke.yaml");
    let out = newton()
        .args([
            "workflow",
            "lint",
            &wf.to_string_lossy(),
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains('[') || stdout.contains("No lint issues"),
        "expected JSON array or no-issues message: {stdout}"
    );
}

#[test]
fn integ_workflow_preview_text() {
    let wf = fixture_path("workflows/minimal_smoke.yaml");
    newton()
        .args([
            "workflow",
            "preview",
            &wf.to_string_lossy(),
            "--format",
            "text",
        ])
        .assert()
        .success();
}

#[test]
fn integ_workflow_preview_multi_trigger_json() {
    let wf = fixture_path("workflows/minimal_smoke.yaml");
    let out = newton()
        .args([
            "workflow",
            "preview",
            &wf.to_string_lossy(),
            "--trigger",
            "board_item_id=PVTI_test",
            "--trigger",
            "skip_idea_to_draft=true",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("board_item_id"),
        "expected board_item_id in triggers JSON: {stdout}"
    );
    assert!(
        stdout.contains("skip_idea_to_draft"),
        "expected skip_idea_to_draft in triggers JSON: {stdout}"
    );
}

#[test]
fn integ_workflow_graph_dot() {
    let wf = fixture_path("workflows/minimal_smoke.yaml");
    let out = newton()
        .args(["workflow", "graph", &wf.to_string_lossy()])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("digraph"),
        "expected DOT format output: {stdout}"
    );
}
