use clap::{error::ErrorKind, Parser};
use newton::cli::args::{
    DotArgs, ExplainArgs, LintArgs, OutputFormat, RunArgs, ValidateArgs, WebhookArgs,
    WebhookCommand, WebhookServeArgs, WebhookStatusArgs,
};
use newton::cli::{commands, Args, Command};
use std::path::PathBuf;

fn parse_command(argv: &[&str]) -> Command {
    Args::try_parse_from(argv)
        .unwrap_or_else(|err| panic!("expected parse success for {:?}: {err}", argv))
        .command
}

#[test]
fn run_positional_indices_and_file_override_parse_correctly() {
    let Command::Run(run) = parse_command(&[
        "newton",
        "run",
        "flow-a.yaml",
        "--file",
        "flow-b.yaml",
        "input.txt",
    ]) else {
        panic!("expected run command");
    };

    assert_eq!(run.workflow_positional, Some(PathBuf::from("flow-a.yaml")));
    assert_eq!(run.file, Some(PathBuf::from("flow-b.yaml")));
    assert_eq!(run.input_file, Some(PathBuf::from("input.txt")));
    assert_eq!(
        run.resolved_workflow_path(),
        Some(PathBuf::from("flow-b.yaml"))
    );
}

#[test]
fn commands_accept_positional_workflow_path() {
    let Command::Validate(validate) = parse_command(&["newton", "validate", "flow.yaml"]) else {
        panic!("expected validate command");
    };
    assert_eq!(
        validate.resolved_workflow_path(),
        Some(PathBuf::from("flow.yaml"))
    );

    let Command::Dot(dot) = parse_command(&["newton", "dot", "flow.yaml", "--out", "graph.dot"])
    else {
        panic!("expected dot command");
    };
    assert_eq!(
        dot.resolved_workflow_path(),
        Some(PathBuf::from("flow.yaml"))
    );

    let Command::Lint(lint) = parse_command(&["newton", "lint", "flow.yaml", "--format", "json"])
    else {
        panic!("expected lint command");
    };
    assert_eq!(
        lint.resolved_workflow_path(),
        Some(PathBuf::from("flow.yaml"))
    );

    let Command::Explain(explain) =
        parse_command(&["newton", "explain", "flow.yaml", "--format", "text"])
    else {
        panic!("expected explain command");
    };
    assert_eq!(
        explain.resolved_workflow_path(),
        Some(PathBuf::from("flow.yaml"))
    );
}

#[test]
fn webhook_commands_accept_positional_and_file_forms() {
    let Command::Webhook(WebhookArgs {
        command: WebhookCommand::Serve(serve),
    }) = parse_command(&[
        "newton",
        "webhook",
        "serve",
        "flow.yaml",
        "--workspace",
        ".",
    ])
    else {
        panic!("expected webhook serve command");
    };
    assert_eq!(
        serve.resolved_workflow_path(),
        Some(PathBuf::from("flow.yaml"))
    );

    let Command::Webhook(WebhookArgs {
        command: WebhookCommand::Status(status),
    }) = parse_command(&[
        "newton",
        "webhook",
        "status",
        "flow-a.yaml",
        "--file",
        "flow-b.yaml",
        "--workspace",
        ".",
    ])
    else {
        panic!("expected webhook status command");
    };
    assert_eq!(
        status.resolved_workflow_path(),
        Some(PathBuf::from("flow-b.yaml"))
    );
}

#[test]
fn workflow_flag_is_rejected_by_parser() {
    let err = match Args::try_parse_from(["newton", "validate", "--workflow", "flow.yaml"]) {
        Ok(_) => panic!("expected parser to reject --workflow"),
        Err(err) => err,
    };
    assert_eq!(err.kind(), ErrorKind::UnknownArgument);
    assert!(err.to_string().contains("--workflow"));
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
