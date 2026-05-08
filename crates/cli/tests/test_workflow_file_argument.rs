//! Handler-level tests for workflow-file argument plumbing.
//!
//! The argv-parsing portions of this file used the legacy clap `Args` enum
//! that issue #231 removed; cli-framework owns argv parsing now and is
//! exercised by the integration help tests.  The remaining tests here just
//! verify that the `commands::*` handlers raise the expected error when no
//! workflow file is supplied.

use newton_cli::cli::args::{
    DotArgs, ExplainArgs, LintArgs, OutputFormat, RunArgs, ValidateArgs, WebhookArgs,
    WebhookCommand, WebhookServeArgs, WebhookStatusArgs,
};
use newton_cli::cli::commands;
use std::path::PathBuf;

#[test]
fn lint_rejects_prose_format() {
    let err = commands::lint(LintArgs {
        workflow_positional: Some(PathBuf::from(
            "tests/fixtures/workflows/01_minimal_success.yaml",
        )),
        file: None,
        format: OutputFormat::Prose,
    })
    .expect_err("expected lint prose format to be rejected");
    assert!(err
        .to_string()
        .contains("prose format is not supported for lint command"));
}

#[tokio::test]
async fn run_missing_workflow_returns_custom_error() {
    let args = RunArgs {
        workflow_positional: None,
        input_file: None,
        file: None,
        workspace: None,
        arg: Vec::new(),
        set: Vec::new(),
        trigger_json: None,
        parallel_limit: None,
        max_time_seconds: None,
        verbose: false,
        server: None,
    };

    let err = commands::run(args)
        .await
        .expect_err("expected missing workflow error");
    assert!(err
        .to_string()
        .contains("missing workflow file; pass WORKFLOW or --file PATH"));
}

#[test]
fn required_workflow_commands_return_custom_error_when_missing() {
    let validate_err = commands::validate(ValidateArgs {
        workflow_positional: None,
        file: None,
    })
    .expect_err("expected validate missing workflow error");
    assert!(validate_err
        .to_string()
        .contains("missing workflow file; pass WORKFLOW or --file PATH"));

    let dot_err = commands::dot(DotArgs {
        workflow_positional: None,
        file: None,
        out: None,
    })
    .expect_err("expected dot missing workflow error");
    assert!(dot_err
        .to_string()
        .contains("missing workflow file; pass WORKFLOW or --file PATH"));

    let lint_err = commands::lint(LintArgs {
        workflow_positional: None,
        file: None,
        format: OutputFormat::Text,
    })
    .expect_err("expected lint missing workflow error");
    assert!(lint_err
        .to_string()
        .contains("missing workflow file; pass WORKFLOW or --file PATH"));

    let explain_err = commands::explain(ExplainArgs {
        workflow_positional: None,
        file: None,
        workspace: None,
        set: Vec::new(),
        arg: Vec::new(),
        format: OutputFormat::Text,
        trigger_json: None,
    })
    .expect_err("expected explain missing workflow error");
    assert!(explain_err
        .to_string()
        .contains("missing workflow file; pass WORKFLOW or --file PATH"));
}

#[tokio::test]
async fn webhook_serve_missing_workflow_returns_custom_error() {
    let args = WebhookArgs {
        command: WebhookCommand::Serve(WebhookServeArgs {
            workflow_positional: None,
            file: None,
            workspace: PathBuf::from("."),
        }),
    };

    let err = commands::webhook(args)
        .await
        .expect_err("expected missing workflow error");
    assert!(err
        .to_string()
        .contains("missing workflow file; pass WORKFLOW or --file PATH"));
}

#[tokio::test]
async fn webhook_status_auto_discovery_error_mentions_new_contract() {
    let workspace = tempfile::tempdir().expect("create temp workspace");
    let args = WebhookArgs {
        command: WebhookCommand::Status(WebhookStatusArgs {
            workflow_positional: None,
            file: None,
            workspace: workspace.path().to_path_buf(),
        }),
    };

    let err = commands::webhook(args)
        .await
        .expect_err("expected auto-discovery error");
    assert!(err
        .to_string()
        .contains("pass WORKFLOW or --file PATH to specify"));
}
