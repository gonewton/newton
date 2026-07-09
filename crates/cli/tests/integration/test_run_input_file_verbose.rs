//! P5a/P5b (spec 074 tranche 2, decision 9): `workflow run`'s INPUT_FILE
//! positional and `--verbose` flag were parsed but never wired to anything.
//! INPUT_FILE must land in `triggers.payload.input_file` (and fail cleanly,
//! not panic, if the file doesn't exist); `--verbose` must print each
//! completed task's captured stdout/stderr to the terminal.

#[path = "../support/mod.rs"]
mod support;

use support::{fixture_path, newton, TempWorkspace};

/// P5a: INPUT_FILE is injected into `triggers.payload.input_file` and is
/// visible to the workflow (here, echoed by a `CommandOperator` into a
/// captured-output file so the assertion doesn't depend on `--verbose`).
#[test]
fn run_input_file_injected_into_trigger_payload() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/input_file_echo.yaml");
    let input_file = ws.path().join("input.txt");
    std::fs::write(&input_file, "hello from the input file").expect("write input file");

    let out = newton()
        .args([
            "workflow",
            "run",
            &wf.to_string_lossy(),
            &input_file.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
        ])
        .output()
        .expect("newton run should execute");

    assert!(
        out.status.success(),
        "run should succeed; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let captured = std::fs::read_to_string(ws.path().join("input_file_captured.txt"))
        .expect("task should have written its captured stdout");
    assert_eq!(
        captured,
        format!("INPUT_FILE={}", input_file.display()),
        "captured stdout should contain the exact INPUT_FILE positional path"
    );
}

/// P5a: a missing INPUT_FILE must fail cleanly (non-zero exit, no panic
/// backtrace), not crash the process.
#[test]
fn run_input_file_missing_fails_cleanly_not_panic() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/input_file_echo.yaml");
    let missing = ws.path().join("does-not-exist.txt");

    let out = newton()
        .args([
            "workflow",
            "run",
            &wf.to_string_lossy(),
            &missing.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
        ])
        .output()
        .expect("newton run should execute (even with a missing input file)");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        !out.status.success(),
        "run with a missing INPUT_FILE must not succeed; stdout={stdout} stderr={stderr}"
    );
    assert!(
        out.status.code().is_some(),
        "process must exit with a code (clean error), not be killed by a signal (panic/abort); status={:?}",
        out.status
    );
    assert!(
        !combined.contains("panicked at"),
        "must be a clean error, not a Rust panic: {combined}"
    );
    assert!(
        combined.contains("input file not found") || combined.contains("WFG-IO-006"),
        "expected a clean 'input file not found' error message: {combined}"
    );
}

/// P5a: `--emit-completion-json` on a missing INPUT_FILE emits a structured
/// internal_error envelope with the WFG-IO-006 code and exits 1, matching
/// the tranche-1 `emit_or_return`/`CliExit` conventions used by the other
/// pre-flight validation errors in `execute_run_command`.
#[test]
fn run_input_file_missing_emit_completion_json_internal_error() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/input_file_echo.yaml");
    let missing = ws.path().join("does-not-exist.txt");

    let out = newton()
        .args([
            "workflow",
            "run",
            &wf.to_string_lossy(),
            &missing.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
            "--emit-completion-json",
        ])
        .output()
        .expect("newton run should execute");

    assert_eq!(
        out.status.code(),
        Some(1),
        "exit code should be 1 for WFG-IO-006; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let envelope: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be valid JSON: {e}; stdout={stdout}"));
    assert_eq!(
        envelope["status"], "internal_error",
        "status must be 'internal_error'; envelope={envelope}"
    );
    assert_eq!(
        envelope["error"]["code"], "WFG-IO-006",
        "error code must be WFG-IO-006; envelope={envelope}"
    );
}

/// P5b: `--verbose` prints the completed task's captured stdout to the
/// terminal, with a per-task header.
#[test]
fn verbose_prints_captured_task_output_to_terminal() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/verbose_marker.yaml");

    let out = newton()
        .args([
            "workflow",
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
            "--verbose",
        ])
        .output()
        .expect("newton run should execute");

    assert!(
        out.status.success(),
        "run should succeed; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("VERBOSE_MARKER_9f3a1c"),
        "expected the task's captured stdout marker in --verbose output: {stdout}"
    );
    assert!(
        stdout.contains("echo_marker"),
        "expected a per-task header naming the task id: {stdout}"
    );
}

/// P5b: without `--verbose`, the captured marker must not appear on the
/// terminal — behavior is unchanged when the flag is absent.
#[test]
fn without_verbose_marker_absent_from_terminal() {
    let ws = TempWorkspace::new();
    let wf = fixture_path("workflows/verbose_marker.yaml");

    let out = newton()
        .args([
            "workflow",
            "run",
            &wf.to_string_lossy(),
            "--workspace",
            &ws.path().to_string_lossy(),
        ])
        .output()
        .expect("newton run should execute");

    assert!(
        out.status.success(),
        "run should succeed; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stdout.contains("VERBOSE_MARKER_9f3a1c") && !stderr.contains("VERBOSE_MARKER_9f3a1c"),
        "marker must not appear without --verbose: stdout={stdout} stderr={stderr}"
    );
}
