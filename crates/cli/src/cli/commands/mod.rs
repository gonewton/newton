#![allow(clippy::result_large_err)]

pub mod artifact;
pub mod batch;
pub mod checkpoint;
pub mod data;
pub mod import;
pub mod log;
pub mod serve;
pub mod webhook;
pub mod workflow;

use crate::cli::args::{KeyValuePair, RunArgs};
use crate::cli::workspace_paths::{state_artifacts_dir, state_checkpoints_dir};
use crate::Result;
use anyhow::anyhow;
use newton_core::core::error::AppError;
use newton_core::core::types::ErrorCategory;
use newton_core::workflow::operator::OperatorRegistry;
use newton_core::workflow::{
    executor::ExecutionOverrides,
    explain as workflow_explain,
    expression::ExpressionEngine,
    lint::{LintRegistry, LintResult, LintSeverity},
    operators as workflow_operators, schema as workflow_schema, transform as workflow_transform,
    workflow_sink::WorkflowSink,
};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
    result::Result as StdResult,
    sync::Arc,
    time::Duration,
};

pub use artifact::artifacts;
pub use batch::batch;
pub use checkpoint::checkpoints;
pub use data::data;
pub use import::workflow_import;
pub use log::log;
pub use serve::serve;
pub use webhook::webhook;
pub use workflow::{dot, explain, lint, resume, run, validate, workflow_run};

fn resolve_workflow_workspace(path: Option<PathBuf>) -> StdResult<PathBuf, AppError> {
    match path {
        Some(p) => Ok(p),
        None => Ok(env::current_dir().map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to resolve workspace path: {err}"),
            )
        })?),
    }
}

fn resolve_workspace_workflow_path(
    workspace: &Path,
    override_path: Option<PathBuf>,
) -> StdResult<PathBuf, AppError> {
    if let Some(path) = override_path {
        return Ok(path);
    }
    for candidate in &["workflow.yaml", "workflow.yml"] {
        let candidate_path = workspace.join(candidate);
        if candidate_path.exists() {
            return Ok(candidate_path);
        }
    }
    Err(AppError::new(
        ErrorCategory::ValidationError,
        format!(
            "workflow file not found under {}; pass WORKFLOW or --file PATH to specify",
            workspace.display()
        ),
    ))
}

fn build_operator_registry(
    workspace: PathBuf,
    settings: &workflow_schema::WorkflowSettings,
    ailoop_ctx: Option<newton_core::integrations::ailoop::AiloopContext>,
) -> OperatorRegistry {
    let mut builder = OperatorRegistry::builder();
    let interviewer = newton_core::workflow::human::lazy_interviewer_provider(
        ailoop_ctx,
        Duration::from_secs(settings.human.default_timeout_seconds),
    );
    workflow_operators::register_builtins_with_deps(
        &mut builder,
        workspace,
        settings.clone(),
        workflow_operators::BuiltinOperatorDeps {
            interviewer: Some(interviewer),
            ..Default::default()
        },
    );
    builder.build()
}

#[allow(dead_code)]
fn load_and_validate_workflow(
    args: &RunArgs,
) -> Result<(workflow_schema::WorkflowDocument, PathBuf)> {
    let workflow_path = args.workflow.clone();
    let raw_document = workflow_schema::parse_workflow(&workflow_path)?;
    let mut document = workflow_transform::apply_default_pipeline(raw_document)?;

    let lint_results = LintRegistry::new().run(&document);
    if !lint_results.is_empty() {
        print_lint_results_text(&lint_results);
    }
    let error_count = lint_results
        .iter()
        .filter(|result| result.severity == LintSeverity::Error)
        .count();
    if error_count > 0 {
        return Err(anyhow!(
            "workflow lint detected {error_count} error(s); fix before running"
        ));
    }

    apply_context_overrides(&mut document.workflow.context, &args.context);
    document.validate(&ExpressionEngine::default())?;

    Ok((document, workflow_path))
}

#[allow(dead_code)]
fn build_comprehensive_trigger_payload(
    args: &RunArgs,
    workspace: &std::path::Path,
) -> Result<Option<Value>> {
    let mut trigger_payload =
        build_trigger_payload(&args.parameters_json, &args.trigger)?.unwrap_or_else(|| json!({}));

    if let Some(input_file) = &args.input_file {
        let input_file_path = if input_file.is_absolute() {
            input_file.clone()
        } else {
            std::env::current_dir()?.join(input_file)
        };
        trigger_payload["input_file"] = json!(input_file_path.display().to_string());
    }

    trigger_payload["workspace"] = json!(workspace.display().to_string());

    if trigger_payload.as_object().unwrap().is_empty() {
        Ok(None)
    } else {
        Ok(Some(trigger_payload))
    }
}

#[allow(dead_code)]
fn setup_workflow_execution(
    args: &RunArgs,
    workspace: &std::path::Path,
    settings: &workflow_schema::WorkflowSettings,
    state_dir: &std::path::Path,
    sink: Option<Arc<dyn WorkflowSink>>,
) -> (ExecutionOverrides, OperatorRegistry) {
    let overrides = ExecutionOverrides {
        parallel_limit: args.parallel_limit,
        max_time_seconds: args.timeout_seconds,
        checkpoint_base_path: Some(state_checkpoints_dir(state_dir)),
        artifact_base_path: Some(state_artifacts_dir(state_dir)),
        max_nesting_depth: None,
        verbose: args.verbose,
        sink,
        pre_seed_nodes: true,
    };

    let ailoop_ctx =
        newton_core::integrations::ailoop::init_context_for_command_name(workspace, "run")
            .ok()
            .flatten();
    let registry = build_operator_registry(workspace.to_path_buf(), settings, ailoop_ctx);

    (overrides, registry)
}

fn apply_context_overrides(context: &mut Value, overrides: &[KeyValuePair]) {
    if !context.is_object() {
        *context = Value::Object(Map::new());
    }
    if let Some(map) = context.as_object_mut() {
        for pair in overrides {
            let parsed = serde_json::from_str(&pair.value)
                .unwrap_or_else(|_| Value::String(pair.value.clone()));
            map.insert(pair.key.clone(), parsed);
        }
    }
}

fn print_lint_results_text(results: &[LintResult]) {
    for result in results {
        if let Some(location) = &result.location {
            println!(
                "{} {} ({}) : {}",
                result.severity, result.code, location, result.message
            );
        } else {
            println!("{} {} : {}", result.severity, result.code, result.message);
        }
        if let Some(suggestion) = &result.suggestion {
            println!("  Suggestion: {suggestion}");
        }
    }
}

fn print_lint_results_json(results: &[LintResult]) -> StdResult<(), AppError> {
    let payload = json!({ "results": results });
    let serialized = serde_json::to_string_pretty(&payload).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize lint results: {err}"),
        )
    })?;
    println!("{serialized}");
    Ok(())
}

fn print_explain_text(
    output: &workflow_explain::ExplainOutput,
    source_summary: Option<(usize, usize, Vec<String>)>,
) -> StdResult<(), AppError> {
    if let Some((task_count, macro_count, macro_names)) = source_summary {
        if macro_count > 0 {
            println!(
                "Source: {} tasks, {} macro invocations ({})",
                task_count,
                macro_count,
                macro_names.join(", ")
            );
        } else {
            println!("Source: {task_count} tasks, 0 macro invocations");
        }
        println!();
    }
    println!("Effective settings:");
    println!("{}", pretty_json(&output.settings)?);
    println!();
    println!("Initial context:");
    println!("{}", pretty_json(&output.context)?);
    println!();
    println!("Triggers:");
    println!("{}", pretty_json(&output.triggers)?);
    println!();
    println!("Tasks:");
    for task in &output.tasks {
        println!("  {} ({})", task.id, task.operator);
        println!("    Params:");
        println!("      {}", pretty_json(&task.params)?);
        println!("    Transitions:");
        for transition in &task.transitions {
            println!(
                "      - to={} priority={} when={}",
                transition.target, transition.priority, transition.when
            );
        }
    }
    Ok(())
}

fn print_explain_json(output: &workflow_explain::ExplainOutput) -> StdResult<(), AppError> {
    let serialized = serde_json::to_string_pretty(output).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize explain output: {err}"),
        )
    })?;
    println!("{serialized}");
    Ok(())
}

fn print_explain_prose(output: &workflow_explain::ExplainOutput) -> StdResult<(), AppError> {
    let prose = workflow_explain::format_explain_prose(output)?;
    println!("{prose}");
    Ok(())
}

fn pretty_json(value: &impl Serialize) -> StdResult<String, AppError> {
    serde_json::to_string_pretty(value).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize explain section: {err}"),
        )
    })
}

fn parse_set_overrides(pairs: &[KeyValuePair]) -> Vec<(String, Value)> {
    pairs
        .iter()
        .map(|pair| {
            let parsed = serde_json::from_str(&pair.value)
                .unwrap_or_else(|_| Value::String(pair.value.clone()));
            (pair.key.clone(), parsed)
        })
        .collect()
}

fn try_load_trigger_payload(path: &Option<PathBuf>) -> StdResult<Option<Value>, AppError> {
    match path {
        Some(path) => Ok(Some(load_trigger_payload(path)?)),
        None => Ok(None),
    }
}

pub fn build_trigger_payload(
    trigger_json: &Option<PathBuf>,
    args: &[KeyValuePair],
) -> StdResult<Option<Value>, AppError> {
    if trigger_json.is_none() && args.is_empty() {
        return Ok(None);
    }

    let mut payload =
        try_load_trigger_payload(trigger_json)?.unwrap_or_else(|| Value::Object(Map::new()));
    let map = payload.as_object_mut().ok_or_else(|| {
        AppError::new(
            ErrorCategory::ValidationError,
            "trigger JSON must be an object",
        )
    })?;

    for pair in args {
        map.insert(pair.key.clone(), resolve_trigger_arg_value(&pair.value)?);
    }

    Ok(Some(payload))
}

fn resolve_trigger_arg_value(value: &str) -> StdResult<Value, AppError> {
    if let Some(path) = value.strip_prefix("@@") {
        return Ok(Value::String(format!("@{path}")));
    }
    if let Some(path) = value.strip_prefix('@') {
        if path.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::ValidationError,
                "trigger arg file path is empty",
            ));
        }
        let content = fs::read_to_string(path).map_err(|err| {
            AppError::new(
                ErrorCategory::IoError,
                format!("failed to read trigger arg file {path}: {err}"),
            )
        })?;
        return Ok(Value::String(content));
    }

    Ok(Value::String(value.to_string()))
}

fn load_trigger_payload(path: &Path) -> StdResult<Value, AppError> {
    let path_str = path.to_str().unwrap_or("");
    let actual_path = if let Some(stripped) = path_str.strip_prefix('@') {
        std::path::Path::new(stripped)
    } else {
        path
    };
    let content = fs::read_to_string(actual_path).map_err(|err| {
        AppError::new(
            ErrorCategory::IoError,
            format!(
                "failed to read trigger JSON {}: {}",
                actual_path.display(),
                err
            ),
        )
    })?;
    let value: Value = serde_json::from_str(&content).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to parse trigger JSON {}: {}", path.display(), err),
        )
    })?;
    if !value.is_object() {
        return Err(AppError::new(
            ErrorCategory::ValidationError,
            "trigger JSON must be an object",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn build_trigger_payload_returns_none_without_inputs() {
        let payload = build_trigger_payload(&None, &[]).expect("build payload");
        assert!(payload.is_none());
    }

    #[test]
    fn build_trigger_payload_merges_json_and_args_last_wins() {
        let temp = tempdir().expect("tempdir");
        let json_path = temp.path().join("trigger.json");
        fs::write(&json_path, r#"{"prompt":"base","env":"dev"}"#).expect("write trigger json");

        let args = vec![
            KeyValuePair {
                key: "prompt".to_string(),
                value: "override".to_string(),
            },
            KeyValuePair {
                key: "new_key".to_string(),
                value: "new_value".to_string(),
            },
        ];
        let payload =
            build_trigger_payload(&Some(json_path), &args).expect("build trigger payload");
        assert_eq!(
            payload.expect("payload"),
            json!({"prompt":"override","env":"dev","new_key":"new_value"})
        );
    }

    #[test]
    fn build_trigger_payload_reads_arg_value_from_file() {
        let temp = tempdir().expect("tempdir");
        let prompt_path = temp.path().join("prompt.md");
        fs::write(&prompt_path, "line1\nline2\n").expect("write prompt");
        let arg_value = format!("@{}", prompt_path.display());
        let args = vec![KeyValuePair {
            key: "prompt".to_string(),
            value: arg_value,
        }];

        let payload = build_trigger_payload(&None, &args)
            .expect("build trigger payload")
            .expect("payload");
        assert_eq!(payload, json!({"prompt":"line1\nline2\n"}));
    }

    #[test]
    fn build_trigger_payload_supports_literal_at_escape() {
        let args = vec![KeyValuePair {
            key: "prompt".to_string(),
            value: "@@literal".to_string(),
        }];
        let payload = build_trigger_payload(&None, &args)
            .expect("build trigger payload")
            .expect("payload");
        assert_eq!(payload, json!({"prompt":"@literal"}));
    }

    #[test]
    fn build_trigger_payload_rejects_empty_file_path() {
        let args = vec![KeyValuePair {
            key: "prompt".to_string(),
            value: "@".to_string(),
        }];
        let err = build_trigger_payload(&None, &args).expect_err("expected error");
        assert!(
            err.to_string().contains("trigger arg file path is empty"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn build_trigger_payload_rejects_non_utf8_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("binary.bin");
        fs::write(&path, [0xff, 0xfe]).expect("write bytes");
        let arg_value = format!("@{}", path.display());
        let args = vec![KeyValuePair {
            key: "prompt".to_string(),
            value: arg_value,
        }];
        let err = build_trigger_payload(&None, &args).expect_err("expected error");
        assert!(
            err.to_string().contains("failed to read trigger arg file"),
            "unexpected error: {}",
            err
        );
    }
}
