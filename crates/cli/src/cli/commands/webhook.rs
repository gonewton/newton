#![allow(clippy::result_large_err)]

use crate::cli::args::{WebhookArgs, WebhookCommand, WebhookServeArgs, WebhookStatusArgs};
use newton_core::core::error::AppError;

use newton_core::workflow::{
    executor::ExecutionOverrides, expression::ExpressionEngine, schema as workflow_schema,
    transform as workflow_transform, webhook,
};
use std::{env, result::Result as StdResult};

pub async fn webhook(args: WebhookArgs) -> StdResult<(), AppError> {
    match args.command {
        WebhookCommand::Serve(serve_args) => workflow_webhook_serve(serve_args).await,
        WebhookCommand::Status(status_args) => workflow_webhook_status(status_args),
    }
}

async fn workflow_webhook_serve(args: WebhookServeArgs) -> StdResult<(), AppError> {
    let workflow_path = args.workflow.clone();
    let workspace = super::resolve_workflow_workspace(Some(args.workspace))?;
    let (document, lint_results) =
        newton_core::workflow::loader::load_and_lint_workflow(&workflow_path)?;
    if !lint_results.is_empty() {
        super::print_lint_results_text(&lint_results);
    }
    document.validate(&ExpressionEngine::default())?;

    let settings = document.workflow.settings.clone();
    let registry = super::build_operator_registry(workspace.clone(), &settings, None);
    let overrides = ExecutionOverrides {
        parallel_limit: None,
        max_time_seconds: None,
        checkpoint_base_path: None,
        artifact_base_path: None,
        max_nesting_depth: None,
        verbose: false,
        sink: None,
        pre_seed_nodes: true,
    };

    webhook::serve_webhook(
        document,
        workflow_path,
        registry,
        workspace.clone(),
        overrides,
    )
    .await?;
    Ok(())
}

fn workflow_webhook_status(args: WebhookStatusArgs) -> StdResult<(), AppError> {
    let workspace = super::resolve_workflow_workspace(Some(args.workspace))?;
    let workflow_path = super::resolve_workspace_workflow_path(&workspace, args.workflow)?;
    let raw_document = workflow_schema::parse_workflow(&workflow_path)?;
    let document = workflow_transform::apply_default_pipeline(raw_document)?;
    let settings = document.workflow.settings.webhook;
    if !settings.enabled {
        println!("Webhook not configured.");
        return Ok(());
    }
    let token_set = env::var(&settings.auth_token_env)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    println!("{:<16} {}", "Bind address:", settings.bind);
    println!(
        "{:<16} {} (set: {})",
        "Auth token env:",
        settings.auth_token_env,
        if token_set { "yes" } else { "no" }
    );
    println!("{:<16} {}", "Max body bytes:", settings.max_body_bytes);
    Ok(())
}
