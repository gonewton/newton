#[path = "../support/mod.rs"]
mod support;

use support::{fixture_path, newton, TempWorkspace};

/// AC 9: success with --emit-completion-json writes valid JSON to stdout; exit 0.
#[test]
fn emit_completion_json_success_exit_0() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/io_contract_success.yaml");

    let out = newton()
        .args([
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
            "--emit-completion-json",
        ])
        .output()
        .expect("newton run should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "exit code should be 0 on success; stdout={stdout}, stderr={stderr}"
    );
    let envelope: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be valid JSON: {e}; stdout={stdout}"));
    assert_eq!(
        envelope["schema_version"], "1",
        "schema_version must be '1'"
    );
    assert_eq!(envelope["status"], "success", "status must be 'success'");
    assert!(envelope["result"].is_object(), "result must be an object");
    assert!(envelope["error"].is_null(), "error must be null on success");
}

/// AC 12: without --emit-completion-json, stdout is human-readable only.
#[test]
fn no_emit_completion_json_human_readable() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/io_contract_success.yaml");

    let out = newton()
        .args([
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
        ])
        .output()
        .expect("newton run should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "run should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Without the flag, stdout must NOT be a JSON object envelope.
    assert!(
        !stdout.trim().starts_with('{'),
        "stdout should be human-readable, not JSON; stdout={stdout}"
    );
    assert!(
        stdout.contains("completed") || stdout.contains("iterations"),
        "stdout should contain human-readable completion message; stdout={stdout}"
    );
}

/// AC 10: workflow failure (output_schema mismatch) exits with code 2;
/// JSON envelope has status=failure.
#[test]
fn emit_completion_json_workflow_failure_exit_2() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/io_contract_failure.yaml");

    let out = newton()
        .args([
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
            "--emit-completion-json",
        ])
        .output()
        .expect("newton run should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "exit code should be 2 on workflow failure; stdout={stdout}, stderr={stderr}"
    );
    let envelope: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be valid JSON: {e}; stdout={stdout}"));
    assert_eq!(
        envelope["status"], "failure",
        "status must be 'failure'; envelope={envelope}"
    );
    assert!(
        envelope["error"].is_object(),
        "error must be present on failure; envelope={envelope}"
    );
}

/// AC 11: internal error (WFG-IO-002 from missing required input) exits with
/// code 1; JSON envelope has status=internal_error.
#[test]
fn emit_completion_json_internal_error_exit_1() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/io_contract_input_schema.yaml");

    // Run WITHOUT providing the required `repo` parameter — triggers WFG-IO-002.
    let out = newton()
        .args([
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
            "--emit-completion-json",
        ])
        .output()
        .expect("newton run should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "exit code should be 1 on internal error; stdout={stdout}, stderr={stderr}"
    );
    let envelope: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be valid JSON: {e}; stdout={stdout}"));
    assert_eq!(
        envelope["status"], "internal_error",
        "status must be 'internal_error'; envelope={envelope}"
    );
    assert_eq!(
        envelope["error"]["code"], "WFG-IO-002",
        "error code must be WFG-IO-002; envelope={envelope}"
    );
    assert!(
        envelope["result"].is_null(),
        "result must be null on internal error; envelope={envelope}"
    );
}

/// AC 13: --parameters-json loads the JSON file as base trigger payload.
#[test]
fn parameters_json_loads_trigger_payload() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/io_contract_input_schema.yaml");

    // Write a params file with the required `repo` field.
    let params_file = ws.path().join("params.json");
    std::fs::write(&params_file, r#"{"repo": "my-repo"}"#).expect("write params");

    let out = newton()
        .args([
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
            "--parameters-json",
            &params_file.to_string_lossy(),
        ])
        .output()
        .expect("newton run should execute");

    assert!(
        out.status.success(),
        "run with valid --parameters-json should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// AC 14: --trigger-file is not recognized; CLI prints an error mentioning the flag.
#[test]
fn trigger_file_flag_rejected() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/minimal_smoke.yaml");

    let out = newton()
        .args([
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
            "--trigger-file",
            "params.json",
        ])
        .output()
        .expect("newton run should execute (even with bad flag)");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // The CLI must either fail or print an error. The old --trigger-file flag must not
    // silently succeed: either the process exits non-zero or the output contains an
    // error message indicating the flag is not recognized.
    let rejected = !out.status.success()
        || combined.contains("trigger-file")
        || combined.contains("unrecognized")
        || combined.contains("unexpected")
        || combined.contains("unknown argument");
    assert!(
        rejected,
        "--trigger-file should be rejected or produce an error; got exit={:?}, output={combined}",
        out.status.code()
    );
}

/// AC 15: --parameters-json @path syntax resolves same as bare path.
#[test]
fn parameters_json_at_prefix_resolves_file() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/io_contract_input_schema.yaml");

    let params_file = ws.path().join("params.json");
    std::fs::write(&params_file, r#"{"repo": "my-repo"}"#).expect("write params");
    let at_path = format!("@{}", params_file.to_string_lossy());

    let out = newton()
        .args([
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
            "--parameters-json",
            &at_path,
        ])
        .output()
        .expect("newton run should execute");

    assert!(
        out.status.success(),
        "run with @path --parameters-json should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// AC 4 / AC 19: WFG-IO-001 is emitted when the trigger payload exceeds max_input_bytes.
/// The fixture has max_input_bytes: 1 so any non-empty params file will exceed the limit.
#[test]
fn wfg_io_001_emitted_when_payload_exceeds_max_input_bytes() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/io_contract_max_input_bytes.yaml");

    // Write a params file — any non-trivial JSON exceeds max_input_bytes: 1
    let params_file = ws.path().join("params.json");
    std::fs::write(&params_file, r#"{"repo": "my-repo"}"#).expect("write params");

    let out = newton()
        .args([
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
            "--parameters-json",
            &params_file.to_string_lossy(),
            "--emit-completion-json",
        ])
        .output()
        .expect("newton run should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "exit code should be 1 for WFG-IO-001; stdout={stdout}, stderr={stderr}"
    );
    let envelope: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be valid JSON: {e}; stdout={stdout}"));
    assert_eq!(
        envelope["status"], "internal_error",
        "status must be 'internal_error' for WFG-IO-001; envelope={envelope}"
    );
    assert_eq!(
        envelope["error"]["code"], "WFG-IO-001",
        "error code must be WFG-IO-001; envelope={envelope}"
    );
}

/// AC 20: WFG-IO-003 is emitted when the serialized result exceeds max_output_bytes.
/// The fixture has max_output_bytes: 1 and a result_map with a non-trivial result.
#[test]
fn wfg_io_003_emitted_when_result_exceeds_max_output_bytes() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/io_contract_max_output_bytes.yaml");

    let out = newton()
        .args([
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
            "--emit-completion-json",
        ])
        .output()
        .expect("newton run should execute");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "exit code should be 2 for WFG-IO-003 (output size exceeded); stdout={stdout}, stderr={stderr}"
    );
    let envelope: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be valid JSON: {e}; stdout={stdout}"));
    assert_eq!(
        envelope["status"], "failure",
        "status must be 'failure' for WFG-IO-003; envelope={envelope}"
    );
    assert_eq!(
        envelope["error"]["code"], "WFG-IO-003",
        "error code must be WFG-IO-003; envelope={envelope}"
    );
}
