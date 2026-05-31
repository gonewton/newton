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

use crate::cli::args::KeyValuePair;
use newton_core::core::error::AppError;
use newton_core::core::types::ErrorCategory;
use newton_core::workflow::operator::OperatorRegistry;
use newton_core::workflow::{
    explain as workflow_explain, lint::LintResult, operators as workflow_operators,
    schema as workflow_schema,
};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
    result::Result as StdResult,
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
pub use workflow::{dot, explain, lint, resume, validate, workflow_run};

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

fn parse_kvp_value(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_owned()))
}

fn apply_context_overrides(context: &mut Value, overrides: &[KeyValuePair]) {
    if !context.is_object() {
        *context = Value::Object(Map::new());
    }
    if let Some(map) = context.as_object_mut() {
        for pair in overrides {
            map.insert(pair.key.clone(), parse_kvp_value(&pair.value));
        }
    }
}

fn print_lint_results_text(results: &[LintResult]) -> StdResult<(), AppError> {
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
    Ok(())
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
        .map(|pair| (pair.key.clone(), parse_kvp_value(&pair.value)))
        .collect()
}

pub fn build_trigger_payload(
    trigger_json: &Option<PathBuf>,
    args: &[KeyValuePair],
) -> StdResult<Option<Value>, AppError> {
    if trigger_json.is_none() && args.is_empty() {
        return Ok(None);
    }

    let mut payload = trigger_json
        .as_ref()
        .map(|p| load_trigger_payload(p))
        .transpose()?
        .unwrap_or_else(|| Value::Object(Map::new()));
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
