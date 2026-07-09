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
