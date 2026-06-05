#[path = "../support/mod.rs"]
mod support;

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
