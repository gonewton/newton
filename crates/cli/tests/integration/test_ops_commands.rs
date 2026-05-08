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
fn health_with_empty_version_returns_cli_ops_001() {
    let err = newton_cli::ops::health::run_with_version("")
        .expect_err("empty version must surface CLI-OPS-001");
    assert!(
        format!("{err}").contains("CLI-OPS-001"),
        "expected CLI-OPS-001 in: {err}"
    );
}

#[test]
fn doctor_workspace_probe_failure_surfaces_cli_ops_002() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Pass a workspace without `.newton/`; probe_workspace_writable will fail
    // because writing `<ws>/.newton/.doctor-probe` requires the parent dir.
    let report = newton_cli::ops::doctor::run(newton_cli::ops::doctor::DoctorArgs {
        workspace: Some(dir.path().to_path_buf()),
    })
    .expect("doctor run returns Ok with FAIL probes inside report");
    let workspace_probe = report
        .probes
        .iter()
        .find(|p| p.name == "workspace")
        .expect("workspace probe present");
    assert_eq!(
        workspace_probe.status,
        newton_cli::ops::doctor::ProbeStatus::Fail
    );
    assert!(
        workspace_probe.detail.contains("CLI-OPS-002"),
        "expected CLI-OPS-002, got: {}",
        workspace_probe.detail
    );
}

#[test]
fn doctor_ailoop_probe_failure_surfaces_cli_ops_003() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg_dir = dir.path().join(".newton/configs");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    // Port 1 is reserved/closed everywhere; the connect_timeout will fail.
    std::fs::write(
        cfg_dir.join("monitor.conf"),
        "ailoop_server_http_url=http://127.0.0.1:1\n",
    )
    .unwrap();
    let report = newton_cli::ops::doctor::run(newton_cli::ops::doctor::DoctorArgs {
        workspace: Some(dir.path().to_path_buf()),
    })
    .expect("doctor run produces a report");
    let ailoop_probe = report
        .probes
        .iter()
        .find(|p| p.name == "ailoop")
        .expect("ailoop probe present");
    assert_eq!(
        ailoop_probe.status,
        newton_cli::ops::doctor::ProbeStatus::Fail,
        "ailoop probe should fail, detail: {}",
        ailoop_probe.detail
    );
    assert!(
        ailoop_probe.detail.contains("CLI-OPS-003"),
        "expected CLI-OPS-003, got: {}",
        ailoop_probe.detail
    );
}

#[test]
fn config_show_missing_workspace_surfaces_cli_ops_004() {
    let bogus = std::path::PathBuf::from("/definitely/not/a/real/newton/workspace/cli-ops-004");
    let err = newton_cli::ops::config_show::run(newton_cli::ops::config_show::ConfigShowArgs {
        workspace: Some(bogus),
    })
    .expect_err("nonexistent workspace must error");
    assert!(
        format!("{err}").contains("CLI-OPS-004"),
        "expected CLI-OPS-004 in: {err}"
    );
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
