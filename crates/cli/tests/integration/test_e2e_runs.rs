#[path = "../support/mod.rs"]
mod support;

use support::{newton, RunStatus, TempWorkspace};

const RUN_ID_A: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

#[test]
fn integ_runs_list_seeded_workspace() {
    let ws = TempWorkspace::new();
    ws.seed_run(RUN_ID_A, RunStatus::Completed);

    let out = newton()
        .args(["runs", "list", "--workspace", &ws.path().to_string_lossy()])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(RUN_ID_A),
        "runs list should contain seeded run id; got: {stdout}"
    );
}

#[test]
fn integ_runs_list_json() {
    let ws = TempWorkspace::new();
    ws.seed_run(RUN_ID_A, RunStatus::Completed);

    let out = newton()
        .args([
            "runs",
            "list",
            "--workspace",
            &ws.path().to_string_lossy(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("runs list --json must emit valid JSON");
    assert!(
        parsed.is_array(),
        "runs list --json should emit a JSON array; got: {stdout}"
    );
    let arr = parsed.as_array().unwrap();
    assert!(
        arr.iter().any(|item| item
            .get("execution_id")
            .and_then(|v| v.as_str())
            .map(|s| s == RUN_ID_A)
            .unwrap_or(false)),
        "JSON array should contain seeded run; got: {stdout}"
    );
}

#[test]
fn integ_runs_show_seeded_run() {
    let ws = TempWorkspace::new();
    ws.seed_run(RUN_ID_A, RunStatus::Completed);

    let out = newton()
        .args([
            "runs",
            "show",
            RUN_ID_A,
            "--workspace",
            &ws.path().to_string_lossy(),
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(RUN_ID_A),
        "runs show should contain run id; got: {stdout}"
    );
}
