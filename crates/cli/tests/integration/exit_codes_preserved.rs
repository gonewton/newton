//! Spec 074, PR-1 / B3: converting handler `std::process::exit` call sites to
//! `Err(CliExit{..}.into())` (mapped back to `std::process::exit` only in
//! `main.rs`) must not change what a direct CLI invocation observes: same
//! exit code, same stderr content. This file pins the exact exit codes for
//! the two non-workflow files touched by the conversion (`data.rs`,
//! `framework_setup/commands/ops.rs`); `test_e2e_io_contract.rs` already
//! pins the workflow `--emit-completion-json` exit codes (0/1/2) exactly.
#[path = "../support/mod.rs"]
mod support;

use support::newton;

fn setup_workspace_with_db() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".newton/state")).unwrap();
    dir
}

#[test]
fn data_unknown_resource_exits_exactly_1() {
    let dir = setup_workspace_with_db();
    let out = newton()
        .args([
            "data",
            "get",
            "not-a-real-resource",
            "--workspace",
            &dir.path().to_string_lossy(),
        ])
        .output()
        .expect("newton should execute");
    assert_eq!(
        out.status.code(),
        Some(1),
        "DATA-003 unknown resource must exit exactly 1"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("DATA-003"),
        "stderr should mention DATA-003; got: {stderr}"
    );
}

#[test]
fn data_missing_id_exits_exactly_1() {
    let dir = setup_workspace_with_db();
    let out = newton()
        .args([
            "data",
            "get",
            "product",
            "--workspace",
            &dir.path().to_string_lossy(),
        ])
        .output()
        .expect("newton should execute");
    assert_eq!(
        out.status.code(),
        Some(1),
        "DATA-002 missing id must exit exactly 1"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("DATA-002"),
        "stderr should mention DATA-002; got: {stderr}"
    );
}

#[test]
fn data_invalid_json_body_exits_exactly_1() {
    let dir = setup_workspace_with_db();
    let out = newton()
        .args([
            "data",
            "post",
            "product",
            "--workspace",
            &dir.path().to_string_lossy(),
            "--body",
            "{not valid json",
        ])
        .output()
        .expect("newton should execute");
    assert_eq!(
        out.status.code(),
        Some(1),
        "DATA-004 invalid JSON body must exit exactly 1"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("DATA-004"),
        "stderr should mention DATA-004; got: {stderr}"
    );
}

/// Spec 074, B19: `ResumeArgs::from_arg_value_map` used to
/// `panic!("fw bug: invalid run-id UUID: {e}")` on a malformed `--run-id`,
/// which is genuinely user-controllable input (the arg spec only requires a
/// `String`, never validates UUID format). Must now be a clean, non-panicking
/// CLI error.
#[test]
fn workflow_resume_invalid_run_id_exits_cleanly_no_panic() {
    let out = newton()
        .args(["workflow", "resume", "--run-id", "garbage"])
        .output()
        .expect("newton should execute");
    assert_ne!(
        out.status.code(),
        Some(0),
        "an invalid --run-id must not exit 0"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("panicked"),
        "must not panic on invalid --run-id; stderr={stderr}"
    );
    assert!(
        stderr.contains("run-id") || stderr.contains("UUID") || stderr.contains("uuid"),
        "stderr should mention the invalid --run-id; got: {stderr}"
    );
}

/// Companion to the above: omitting `--run-id` entirely on `resume` also
/// used to panic (`panic!("fw bug: --run-id is required")`) rather than
/// producing a clean error, even though `run-id` is `Cardinality::Optional`
/// in the shared `workflow` command spec (it's reused by `runs show`), so a
/// missing value is not a framework-invariant violation.
#[test]
fn workflow_resume_missing_run_id_exits_cleanly_no_panic() {
    let out = newton()
        .args(["workflow", "resume"])
        .output()
        .expect("newton should execute");
    assert_ne!(
        out.status.code(),
        Some(0),
        "a missing --run-id must not exit 0"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("panicked"),
        "must not panic on missing --run-id; stderr={stderr}"
    );
}

/// Spec 074, B19: `RunArgs::from_arg_value_map` used to
/// `panic!("fw bug: invalid trigger: {e}")` when a `--trigger` value had no
/// `=` — again genuinely user-controllable, since `trigger` is a free-form
/// `String` in the arg spec. Must now be a clean, non-panicking CLI error.
#[test]
fn workflow_run_malformed_trigger_exits_cleanly_no_panic() {
    let out = newton()
        .args(["workflow", "run", "wf.yaml", "--trigger", "foo"])
        .output()
        .expect("newton should execute");
    assert_ne!(
        out.status.code(),
        Some(0),
        "a malformed --trigger (no '=') must not exit 0"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("panicked"),
        "must not panic on malformed --trigger; stderr={stderr}"
    );
    assert!(
        stderr.contains("trigger"),
        "stderr should mention the invalid --trigger; got: {stderr}"
    );
}

/// Sibling to the trigger test above, covering the analogous `--context`
/// parsing path (`RunArgs::try_from_arg_value_map`'s other `parse_kvp_from_map`
/// call), which shared the same `panic!("fw bug: invalid context: {e}")`
/// before the fix.
#[test]
fn workflow_run_malformed_context_exits_cleanly_no_panic() {
    let out = newton()
        .args(["workflow", "run", "wf.yaml", "--context", "not-kvp"])
        .output()
        .expect("newton should execute");
    assert_ne!(
        out.status.code(),
        Some(0),
        "a malformed --context (no '=') must not exit 0"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("panicked"),
        "must not panic on malformed --context; stderr={stderr}"
    );
    assert!(
        stderr.contains("context"),
        "stderr should mention the invalid --context; got: {stderr}"
    );
}

/// `RunArgs::try_from_arg_value_map` also used to panic
/// (`panic!("fw bug: {e}")`) when no workflow file was given at all —
/// `workflow` isn't a `Cardinality::Required` arg in the spec (it's
/// assembled from a positional promotion at the call site), so this is
/// reachable with plain `newton workflow run` and no file argument.
#[test]
fn workflow_run_missing_workflow_file_exits_cleanly_no_panic() {
    let out = newton()
        .args(["workflow", "run"])
        .output()
        .expect("newton should execute");
    assert_ne!(
        out.status.code(),
        Some(0),
        "a missing workflow file must not exit 0"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("panicked"),
        "must not panic on a missing workflow file; stderr={stderr}"
    );
}

#[test]
fn doctor_failing_probe_exits_exactly_1() {
    // A workspace with no `.newton/` at all: `--workspace` makes the
    // workspace probe actually FAIL (as opposed to the SKIP fallback used
    // when no --workspace is given at all and cwd has no .newton either).
    let dir = tempfile::tempdir().unwrap();
    let out = newton()
        .args(["doctor", "--workspace", &dir.path().to_string_lossy()])
        .output()
        .expect("newton should execute");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("FAIL"),
        "expected at least one FAIL probe line; got: {stdout}"
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "doctor with a failing probe must exit exactly 1; stdout={stdout}"
    );
}
