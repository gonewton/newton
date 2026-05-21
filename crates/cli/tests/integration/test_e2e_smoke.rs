//! Spec 301 — Stage 1 smoke tier: every root command id must have a smoke row.
//!
//! All tests invoke the `newton` binary via `assert_cmd::Command::cargo_bin`.

#[path = "../support/mod.rs"]
mod support;

use predicates::prelude::*;
use support::{fixture_path, newton};

#[test]
fn smoke_run_help() {
    // Repurposed (spec 051 Decision #4): exercises the hidden deprecated `newton run` alias.
    // Kept in REQUIRED_SMOKE_IDS until the hidden shim is removed.
    let wf = fixture_path("workflows/minimal_smoke.yaml");
    let out = newton()
        .args(["run", &wf.to_string_lossy()])
        .output()
        .expect("newton run (deprecated) should execute");
    assert!(
        out.status.success(),
        "deprecated newton run should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("[newton] DEPRECATED"),
        "deprecated newton run should emit deprecation notice; stderr={stderr}"
    );
}

#[test]
fn smoke_workflow_run_help() {
    newton()
        .args(["workflow", "run", "--help"])
        .assert()
        .success();
}

#[test]
fn smoke_init_help() {
    newton().args(["init", "--help"]).assert().success();
}

#[test]
fn smoke_batch_help() {
    newton().args(["batch", "--help"]).assert().success();
}

#[test]
fn smoke_serve_help() {
    newton().args(["serve", "--help"]).assert().success();
}

#[test]
fn smoke_workflow_help() {
    newton().args(["workflow", "--help"]).assert().success();
}

#[test]
fn smoke_resume_help() {
    newton().args(["resume", "--help"]).assert().success();
}

#[test]
fn smoke_checkpoint_help() {
    newton().args(["checkpoint", "--help"]).assert().success();
}

#[test]
fn smoke_artifact_help() {
    newton().args(["artifact", "--help"]).assert().success();
}

#[test]
fn smoke_webhook_help() {
    newton().args(["webhook", "--help"]).assert().success();
}

#[test]
fn smoke_runs_help() {
    newton().args(["runs", "--help"]).assert().success();
}

#[test]
fn smoke_health() {
    newton().args(["health"]).assert().success();
}

#[test]
fn smoke_doctor_help() {
    newton().args(["doctor", "--help"]).assert().success();
}

#[test]
fn smoke_config_help() {
    newton().args(["config", "--help"]).assert().success();
}

#[test]
fn smoke_completion_help() {
    newton().args(["completion", "--help"]).assert().success();
}

#[test]
fn smoke_ask_help() {
    // `ask` is gated by the `ask` feature flag. Under `--all-features` the
    // command is registered and `--help` exits 0. Without the feature the
    // binary reports it as unknown. Either outcome is acceptable — what is NOT
    // acceptable is a crash or an unrelated error message.
    let out = newton().args(["ask", "--help"]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let ok = out.status.success()
        || stderr.to_lowercase().contains("unknown")
        || stderr.to_lowercase().contains("unrecognized");
    assert!(
        ok,
        "ask --help: expected success or 'unknown'/'unrecognized' in stderr; \
         got status={:?} stderr={stderr}",
        out.status,
    );
}

#[test]
fn smoke_spec_json() {
    let out = newton()
        .args(["spec", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("spec --format json must emit valid JSON");
    assert!(
        parsed.get("commands").is_some(),
        "spec JSON must contain top-level `commands` key; got: {}",
        &stdout[..stdout.len().min(200)]
    );
    // `spec` is provided by cli-framework; its JSON always uses camelCase
    // `schemaVersion`. If the field name changes upstream, this assertion
    // catches the regression rather than silently accepting either casing.
    assert!(
        parsed.get("schemaVersion").is_some(),
        "spec JSON must contain a `schemaVersion` field (camelCase); got: {}",
        &stdout[..stdout.len().min(300)]
    );
}

// --- Negative tests (integration tier) -------------------------------------

#[test]
fn negative_run_unknown_flag() {
    let out = newton()
        .args(["workflow", "run", "--bogus-flag", "workflow.yaml"])
        .output()
        .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .to_lowercase();
    assert!(
        !out.status.success()
            || combined.contains("unknown")
            || combined.contains("unexpected argument")
            || combined.contains("error"),
        "expected error indication for bogus flag; got status={:?} output=`{combined}`",
        out.status
    );
}

#[test]
fn negative_workflow_validate_missing_arg() {
    let out = newton().args(["workflow", "validate"]).output().unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .to_lowercase();
    assert!(
        !out.status.success()
            || combined.contains("required")
            || combined.contains("missing")
            || combined.contains("usage")
            || combined.contains("unrecognized")
            || combined.contains("error"),
        "stderr/stdout should signal missing required positional: {combined}"
    );
}

#[test]
fn negative_runs_show_missing_id() {
    let out = newton().args(["runs", "show"]).output().unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .to_lowercase();
    assert!(
        !out.status.success() || combined.contains("error") || combined.contains("required"),
        "expected error for missing run id; got status={:?} output=`{combined}`",
        out.status
    );
}

// --- Helpful predicate to keep `predicates` import live --------------------
#[test]
fn smoke_help_top_level_lists_spec() {
    newton()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("spec"));
}
