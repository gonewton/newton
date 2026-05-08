//! Integration tests for the org-baseline operational commands added by issue #231.

use assert_cmd::Command;

const BIN: &str = "newton";

#[test]
fn health_prints_ok_line() {
    let expected_prefix = format!("newton OK {}", newton_cli::VERSION);
    Command::cargo_bin(BIN)
        .expect("binary should build")
        .arg("health")
        .assert()
        .success()
        .stdout(predicates::str::starts_with(expected_prefix));
}

#[test]
fn doctor_succeeds_in_empty_tempdir_with_skips() {
    let dir = tempfile::tempdir().expect("tempdir");
    let output = Command::cargo_bin(BIN)
        .expect("binary should build")
        .arg("doctor")
        .current_dir(dir.path())
        .output()
        .expect("ran");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "doctor exited non-zero: {stdout}");
    assert!(
        stdout.contains("SKIP workspace"),
        "expected workspace SKIP, got:\n{stdout}"
    );
    assert!(
        stdout.contains("SKIP ailoop"),
        "expected ailoop SKIP, got:\n{stdout}"
    );
}

#[test]
fn config_show_emits_redacted_json() {
    let output = Command::cargo_bin(BIN)
        .expect("binary should build")
        .args(["config", "show"])
        .env("NEWTON_TEST_TOKEN", "supersecret")
        .output()
        .expect("ran");
    assert!(output.status.success(), "stdout: {:?}", output.stdout);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("config show emits valid JSON");
    assert!(v.get("newton_version").is_some(), "missing newton_version");
    if let Some(env_section) = v.get("env") {
        if let Some(token) = env_section.get("NEWTON_TEST_TOKEN") {
            assert_eq!(token, &serde_json::json!("***REDACTED***"));
        }
    }
}

#[test]
fn completion_bash_first_line_matches_pattern() {
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let output = Command::cargo_bin(BIN)
            .expect("binary should build")
            .args(["completion", shell])
            .output()
            .unwrap_or_else(|e| panic!("ran completion {shell}: {e}"));
        assert!(output.status.success(), "completion {shell} failed");
        let stdout = String::from_utf8(output.stdout).unwrap();
        let first = stdout.lines().next().unwrap_or("");
        let ok = first.starts_with("_newton()")
            || first.starts_with("complete ")
            || first.starts_with("#compdef")
            || first.starts_with("Register-ArgumentCompleter");
        assert!(
            ok,
            "completion {shell}: first line `{first}` did not match expected stubs"
        );
    }
}

#[test]
fn completion_unknown_shell_errors() {
    let output = Command::cargo_bin(BIN)
        .expect("binary should build")
        .args(["completion", "tcsh"])
        .output()
        .expect("ran");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("CLI-OPS-005"),
        "expected CLI-OPS-005 in stderr: {stderr}"
    );
}
