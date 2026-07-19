//! Regression coverage for CLI numeric-bound and `--format` validation.
//!
//! These guard pre-existing bugs where out-of-range numeric args silently
//! wrapped / fell back to a default (via unchecked `as` casts) and an unknown
//! `--format` value silently defaulted to Text. The fix adds declarative
//! `min`/`max` bounds to the ArgSpecs (enforced by the framework's
//! `validate_typed_args` before any constructor runs, emitting `E004`) and
//! makes `parse_output_format` reject unknown formats with `CLI-MIG-002`.

#[path = "../support/mod.rs"]
mod support;

use support::{fixture_path, newton, TempWorkspace};

/// Combined lowercased stdout+stderr for assertion convenience.
fn combined_lower(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .to_lowercase()
}

// ── serve --port bounds (min=1, max=65535) ───────────────────────────────────

#[test]
fn serve_port_above_max_is_rejected() {
    let out = newton()
        .args(["serve", "--port", "99999"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "port 99999 must be rejected");
    let msg = combined_lower(&out);
    assert!(
        msg.contains("maximum") || msg.contains("65535"),
        "expected an above-maximum message, got: {msg}"
    );
}

#[test]
fn serve_port_below_min_is_rejected() {
    let out = newton().args(["serve", "--port", "0"]).output().unwrap();
    assert!(!out.status.success(), "port 0 must be rejected");
    let msg = combined_lower(&out);
    assert!(
        msg.contains("minimum") || msg.contains("--port"),
        "expected a below-minimum message, got: {msg}"
    );
}

/// Positive control: a valid port passes arg validation. We pair it with an
/// invalid bind host so the process fails fast *after* validation rather than
/// leaving a server running — the point is that the failure is NOT an E004
/// range violation, proving 9000 was accepted by the port bounds.
#[test]
fn serve_valid_port_passes_bound_validation() {
    let out = newton()
        .args(["serve", "--port", "9000", "--host", "999.999.999.999"])
        .output()
        .unwrap();
    let msg = combined_lower(&out);
    assert!(
        !msg.contains("e004") && !msg.contains("maximum") && !msg.contains("minimum"),
        "port 9000 must pass range validation; got: {msg}"
    );
}

// ── workflow run --parallel-limit / --timeout (min=1) ─────────────────────────

#[test]
fn workflow_run_parallel_limit_negative_is_rejected() {
    let wf = fixture_path("workflows/minimal_smoke.yaml");
    let out = newton()
        .args([
            "workflow",
            "run",
            &wf.to_string_lossy(),
            "--parallel-limit",
            "-1",
        ])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "--parallel-limit -1 must be rejected"
    );
}

#[test]
fn workflow_run_timeout_zero_is_rejected() {
    let wf = fixture_path("workflows/minimal_smoke.yaml");
    let out = newton()
        .args(["workflow", "run", &wf.to_string_lossy(), "--timeout", "0"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "--timeout 0 must be rejected");
    let msg = combined_lower(&out);
    assert!(
        msg.contains("minimum") || msg.contains("--timeout"),
        "expected a below-minimum message, got: {msg}"
    );
}

// ── optimize --poll-interval (min=1) ──────────────────────────────────────────

#[test]
fn optimize_poll_interval_negative_is_rejected() {
    let out = newton()
        .args(["optimize", "some-project", "--poll-interval", "-1"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "--poll-interval -1 must be rejected");
}

// ── workflow runs list --last (min=1) ─────────────────────────────────────────

/// `--last 0` is now rejected by the framework bound (E004), NOT by the removed
/// handler-level `LOG-003` branch. Assert the failure and that the message is
/// the framework's below-minimum message rather than the old LOG-003 text.
#[test]
fn runs_list_last_zero_is_rejected_by_framework() {
    let out = newton()
        .args(["workflow", "runs", "list", "--last", "0"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "--last 0 must be rejected");
    let msg = combined_lower(&out);
    assert!(
        !msg.contains("log-003"),
        "should no longer surface LOG-003 for --last 0; got: {msg}"
    );
    assert!(
        msg.contains("minimum") || msg.contains("--last"),
        "expected a below-minimum message, got: {msg}"
    );
}

/// Positive control: a valid `--last` value parses and the command succeeds
/// against an (empty) seeded workspace.
#[test]
fn runs_list_last_valid_positive_control() {
    let ws = TempWorkspace::new();
    newton()
        .args([
            "workflow",
            "runs",
            "list",
            "--last",
            "5",
            "--workspace",
            &ws.path().to_string_lossy(),
        ])
        .assert()
        .success();
}

// ── --format rejection (lint / preview) ───────────────────────────────────────

#[test]
fn workflow_lint_unknown_format_is_rejected() {
    let wf = fixture_path("workflows/minimal_smoke.yaml");
    let out = newton()
        .args(["workflow", "lint", &wf.to_string_lossy(), "--format", "xml"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "--format xml must be rejected");
    let msg = combined_lower(&out);
    assert!(
        msg.contains("unknown format") && msg.contains("cli-mig-002"),
        "expected an unknown-format CLI-MIG-002 message, got: {msg}"
    );
}

/// Positive control: a valid `--format` value still parses and works.
#[test]
fn workflow_lint_valid_format_json_succeeds() {
    let wf = fixture_path("workflows/minimal_smoke.yaml");
    newton()
        .args([
            "workflow",
            "lint",
            &wf.to_string_lossy(),
            "--format",
            "json",
        ])
        .assert()
        .success();
}
