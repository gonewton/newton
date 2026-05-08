//! Tests for workflow-file argument plumbing under the spec 273 surface.
//!
//! After spec 273, every workflow-centric subcommand requires a positional
//! `<WORKFLOW>` argument; `--file` no longer parses anywhere.  The handler-
//! level tests below ensure the typed args still drive the handlers
//! correctly, and that lint's prose format remains explicitly rejected.

use newton_cli::cli::args::{LintArgs, OutputFormat};
use newton_cli::cli::commands;
use std::path::PathBuf;

#[test]
fn lint_rejects_prose_format() {
    let err = commands::lint(LintArgs {
        workflow: PathBuf::from("tests/fixtures/workflows/01_minimal_success.yaml"),
        format: OutputFormat::Prose,
    })
    .expect_err("expected lint prose format to be rejected");
    assert!(err
        .to_string()
        .contains("prose format is not supported for lint command"));
}

// --- §7 criterion 4: legacy flag spellings on `run` MUST NOT parse ---

fn assert_unrecognized(args: &[&str]) {
    use assert_cmd::prelude::*;
    use std::process::Command;
    let mut cmd = Command::cargo_bin("newton").expect("newton binary");
    cmd.args(args);
    let output = cmd.output().expect("spawn newton");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("unrecognized")
            || combined.contains("unexpected")
            || combined.contains("error"),
        "expected `newton {}` to report unrecognized/error; stderr=\n{stderr}\nstdout=\n{stdout}",
        args.join(" ")
    );
}

#[test]
fn legacy_run_flags_are_rejected() {
    for legacy_flag in [
        "--arg",
        "--set",
        "--trigger-json",
        "--max-time-seconds",
        "--file",
    ] {
        assert_unrecognized(&["run", "workflow.yaml", legacy_flag, "x=y"]);
    }
}

// --- §7 criterion 4: other legacy flag renames MUST NOT parse ---

#[test]
fn legacy_resume_execution_id_rejected() {
    assert_unrecognized(&[
        "resume",
        "--execution-id",
        "00000000-0000-0000-0000-000000000000",
    ]);
}

#[test]
fn legacy_misc_flags_rejected() {
    assert_unrecognized(&["serve", "--ui-dir", "./ui"]);
    assert_unrecognized(&["monitor", "--http-url", "http://x"]);
    assert_unrecognized(&["monitor", "--ws-url", "ws://x"]);
    assert_unrecognized(&["monitor", "--backend"]);
    assert_unrecognized(&["batch", "p", "--sleep", "30"]);
    assert_unrecognized(&["init", "--template-source", "x/y"]);
    assert_unrecognized(&["checkpoint", "list", "--workspace", ".", "--format-json"]);
    assert_unrecognized(&["workflow", "graph", "wf.yaml", "--out", "g.dot"]);
}

// --- §7 criterion 5: trigger merge precedence
//   --trigger-file base.json --trigger a=1 --trigger b=@extra.txt
//   yields {"a":"1","b":"hello"} ---

#[test]
fn trigger_payload_merge_precedence() {
    use newton_cli::cli::args::KeyValuePair;
    use newton_cli::cli::commands::build_trigger_payload;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    let dir = tempdir().expect("tempdir");
    let base = dir.path().join("base.json");
    fs::write(&base, r#"{"a":0,"b":"x"}"#).expect("write base");
    let extra = dir.path().join("extra.txt");
    fs::write(&extra, "hello").expect("write extra");

    let trigger = vec![
        KeyValuePair {
            key: "a".to_string(),
            value: "1".to_string(),
        },
        KeyValuePair {
            key: "b".to_string(),
            value: format!("@{}", extra.display()),
        },
    ];

    let payload = build_trigger_payload(&Some(base), &trigger)
        .expect("build_trigger_payload")
        .expect("payload present");

    assert_eq!(payload, json!({"a": "1", "b": "hello"}));
}

// --- §7 criterion 4: additional legacy spellings on workflow.* and webhook ---

#[test]
fn legacy_workflow_subcommand_flags_rejected() {
    assert_unrecognized(&["workflow", "lint", "wf.yaml", "--file", "wf.yaml"]);
    assert_unrecognized(&["workflow", "validate", "wf.yaml", "--file", "wf.yaml"]);
    assert_unrecognized(&["workflow", "preview", "wf.yaml", "--arg", "x=y"]);
    assert_unrecognized(&["workflow", "preview", "wf.yaml", "--set", "x=y"]);
    assert_unrecognized(&["workflow", "preview", "wf.yaml", "--trigger-json", "x.json"]);
}

#[test]
fn webhook_legacy_file_flag_and_positional_rejected() {
    // `--file` was renamed to `--workflow`.
    assert_unrecognized(&["webhook", "serve", "--file", "wf.yaml", "--workspace", "."]);
    assert_unrecognized(&["webhook", "status", "--file", "wf.yaml", "--workspace", "."]);
    // The positional WORKFLOW slot was removed.
    assert_unrecognized(&["webhook", "serve", "wf.yaml", "--workspace", "."]);
}
