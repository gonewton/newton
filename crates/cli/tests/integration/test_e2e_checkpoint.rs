#[path = "../support/mod.rs"]
mod support;

use support::{newton, RunStatus, TempWorkspace};

const RUN_ID_X: &str = "aaaa0000-0000-0000-0000-000000000001";
const RUN_ID_Y: &str = "aaaa0000-0000-0000-0000-000000000002";

#[test]
fn integ_checkpoint_list_json_two_runs() {
    let ws = TempWorkspace::new();
    ws.seed_run(RUN_ID_X, RunStatus::Completed);
    ws.seed_run(RUN_ID_Y, RunStatus::Failed);

    let out = newton()
        .args([
            "workflow",
            "checkpoint",
            "list",
            "--json",
            "--workspace",
            &ws.path().to_string_lossy(),
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("checkpoint list --json must emit valid JSON");
    assert!(parsed.is_array(), "expected JSON array; got: {stdout}");
    let arr = parsed.as_array().unwrap();
    let ids: Vec<&str> = arr
        .iter()
        .filter_map(|item| item.get("execution_id").and_then(|v| v.as_str()))
        .collect();
    assert!(
        ids.contains(&RUN_ID_X) && ids.contains(&RUN_ID_Y),
        "expected both run ids in checkpoint list; got: {ids:?}"
    );
}

#[test]
fn integ_checkpoint_clean_older_than() {
    let ws = TempWorkspace::new();
    ws.seed_run(RUN_ID_X, RunStatus::Completed);

    let checkpoints_dir = ws
        .path()
        .join(".newton/state/workflows")
        .join(RUN_ID_X)
        .join("checkpoints");
    std::fs::create_dir_all(&checkpoints_dir).unwrap();
    let cp_file = checkpoints_dir.join("checkpoint_v1.json");
    std::fs::write(&cp_file, b"{}").unwrap();

    std::process::Command::new("touch")
        .args(["-t", "202001010000", &cp_file.to_string_lossy()])
        .output()
        .expect("touch to set mtime");

    assert!(cp_file.exists(), "checkpoint history file should exist");

    newton()
        .args([
            "workflow",
            "checkpoint",
            "clean",
            "--workspace",
            &ws.path().to_string_lossy(),
            "--older-than",
            "1s",
        ])
        .assert()
        .success();

    assert!(
        !cp_file.exists(),
        "old checkpoint history file should be removed after clean"
    );
}
