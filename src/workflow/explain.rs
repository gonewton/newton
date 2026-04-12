#![allow(clippy::result_large_err)] // Explain returns AppError to preserve structured evaluation diagnostics.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::expression::{EvaluationContext, ExpressionEngine};
use crate::workflow::schema::{Condition, WorkflowDocument, WorkflowTask};
use serde::Serialize;
use serde_json::{Map, Value};

const RUNTIME_PLACEHOLDER: &str = "(runtime)";

#[derive(Debug, Clone, Serialize)]
pub struct ExplainOutput {
    pub settings: Value,
    pub context: Value,
    pub triggers: Value,
    pub tasks: Vec<ExplainTask>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainTask {
    pub id: String,
    pub operator: String,
    pub params: Value,
    pub transitions: Vec<ExplainTransition>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainTransition {
    pub target: String,
    pub priority: i32,
    pub when: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainDiagnostic {
    pub message: String,
    pub location: Option<String>,
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainOutcome {
    pub output: ExplainOutput,
    pub diagnostics: Vec<ExplainDiagnostic>,
}

impl ExplainOutcome {
    pub fn has_blocking_diagnostics(&self) -> bool {
        self.diagnostics.iter().any(|item| item.blocking)
    }
}

pub fn build_explain_output(
    document: &WorkflowDocument,
    set_overrides: &[(String, Value)],
    triggers: &Value,
) -> Result<ExplainOutput, AppError> {
    Ok(build_explain_outcome(document, set_overrides, triggers)?.output)
}

pub fn build_explain_outcome(
    document: &WorkflowDocument,
    set_overrides: &[(String, Value)],
    triggers: &Value,
) -> Result<ExplainOutcome, AppError> {
    let mut context = document.workflow.context.clone();
    apply_context_set_overrides(&mut context, set_overrides);
    let triggers = triggers.clone();

    let settings = serde_json::to_value(&document.workflow.settings).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize workflow settings: {err}"),
        )
    })?;
    let engine = ExpressionEngine::default();
    let mut diagnostics = Vec::new();

    let tasks = document
        .workflow
        .tasks()
        .map(|task| explain_task(task, &context, &triggers, &engine, &mut diagnostics))
        .collect::<Result<Vec<_>, AppError>>()?;

    Ok(ExplainOutcome {
        output: ExplainOutput {
            settings,
            context,
            triggers,
            tasks,
        },
        diagnostics,
    })
}

fn explain_task(
    task: &WorkflowTask,
    context: &Value,
    triggers: &Value,
    engine: &ExpressionEngine,
    diagnostics: &mut Vec<ExplainDiagnostic>,
) -> Result<ExplainTask, AppError> {
    let eval_ctx =
        EvaluationContext::new(context.clone(), Value::Object(Map::new()), triggers.clone());
    let params = resolve_for_explain(
        &task.params,
        &eval_ctx,
        task.id.as_str(),
        engine,
        diagnostics,
    )?;

    let mut transitions = task.transitions.clone();
    transitions.sort_by_key(|item| item.priority);
    let transitions = transitions
        .into_iter()
        .map(|transition| {
            let when = match transition.when {
                None => "(always)".to_string(),
                Some(Condition::Bool(flag)) => flag.to_string(),
                Some(Condition::Expr { expr }) => expr,
            };
            ExplainTransition {
                target: transition.to,
                priority: transition.priority,
                when,
            }
        })
        .collect();

    Ok(ExplainTask {
        id: task.id.clone(),
        operator: task.operator.clone(),
        params,
        transitions,
    })
}

fn resolve_for_explain(
    value: &Value,
    eval_ctx: &EvaluationContext,
    task_id: &str,
    engine: &ExpressionEngine,
    diagnostics: &mut Vec<ExplainDiagnostic>,
) -> Result<Value, AppError> {
    match value {
        Value::Object(map) => {
            if map.len() == 1 && map.contains_key("$expr") {
                if let Some(Value::String(expr)) = map.get("$expr") {
                    if expr_depends_on_tasks(expr) {
                        return Ok(Value::String(RUNTIME_PLACEHOLDER.to_string()));
                    }
                    if let Err(err) = engine.compile(expr) {
                        diagnostics.push(ExplainDiagnostic {
                            message: format!(
                                "$expr parse failure in task '{}': {}",
                                task_id, err.message
                            ),
                            location: Some(task_id.to_string()),
                            blocking: true,
                        });
                        return Ok(value.clone());
                    }
                    return match engine.evaluate(expr, eval_ctx) {
                        Ok(resolved) => Ok(resolved),
                        Err(err) => {
                            diagnostics.push(ExplainDiagnostic {
                                message: format!(
                                    "$expr evaluation error in task '{}': {}",
                                    task_id, err.message
                                ),
                                location: Some(task_id.to_string()),
                                blocking: true,
                            });
                            Ok(value.clone())
                        }
                    };
                }
            }

            let mut resolved = Map::new();
            for (key, child) in map {
                resolved.insert(
                    key.clone(),
                    resolve_for_explain(child, eval_ctx, task_id, engine, diagnostics)?,
                );
            }
            Ok(Value::Object(resolved))
        }
        Value::Array(items) => {
            let mut resolved = Vec::with_capacity(items.len());
            for child in items {
                resolved.push(resolve_for_explain(
                    child,
                    eval_ctx,
                    task_id,
                    engine,
                    diagnostics,
                )?);
            }
            Ok(Value::Array(resolved))
        }
        _ => Ok(value.clone()),
    }
}

fn apply_context_set_overrides(context: &mut Value, overrides: &[(String, Value)]) {
    if !context.is_object() {
        *context = Value::Object(Map::new());
    }
    if let Some(map) = context.as_object_mut() {
        for (key, value) in overrides {
            map.insert(key.clone(), value.clone());
        }
    }
}

fn expr_depends_on_tasks(expr: &str) -> bool {
    expr.contains("tasks.") || expr.contains("tasks[")
}

/// Format the ExplainOutput as prose for delegation purposes.
/// This creates a human-readable description that can be used independently
/// of the workflow YAML or Newton runtime.
pub fn format_explain_prose(output: &ExplainOutput) -> Result<String, AppError> {
    let mut prose = String::new();

    // Header and introduction
    format_prose_header(&mut prose);

    // Content sections
    format_context_section(&mut prose, &output.context);
    format_triggers_section(&mut prose, &output.triggers);
    format_settings_section(&mut prose, &output.settings);
    format_tasks_section(&mut prose, &output.tasks);
    format_execution_notes(&mut prose);

    Ok(prose)
}

fn format_prose_header(prose: &mut String) {
    prose.push_str("# Workflow Execution Instructions\n\n");
    prose.push_str("This document contains complete instructions for executing a workflow. ");
    prose.push_str("All steps, conditions, and parameters are included to enable execution ");
    prose.push_str("without access to the original workflow file or Newton runtime.\n\n");
}

fn format_context_section(prose: &mut String, context: &Value) {
    prose.push_str("## Context\n\n");
    match serde_json::to_string_pretty(context) {
        Ok(formatted_context) => {
            prose.push_str("Initial workflow context:\n");
            prose.push_str("```json\n");
            prose.push_str(&formatted_context);
            prose.push_str("\n```\n\n");
        }
        Err(_) => {
            prose.push_str("Initial workflow context: (unable to format)\n\n");
        }
    }
}

fn format_triggers_section(prose: &mut String, triggers: &Value) {
    prose.push_str("## Trigger Information\n\n");
    match serde_json::to_string_pretty(triggers) {
        Ok(formatted_triggers) => {
            prose.push_str("Workflow triggers and payload:\n");
            prose.push_str("```json\n");
            prose.push_str(&formatted_triggers);
            prose.push_str("\n```\n\n");
        }
        Err(_) => {
            prose.push_str("Workflow triggers and payload: (unable to format)\n\n");
        }
    }
}

fn format_settings_section(prose: &mut String, settings: &Value) {
    prose.push_str("## Workflow Settings\n\n");
    match serde_json::to_string_pretty(settings) {
        Ok(formatted_settings) => {
            prose.push_str("Effective workflow settings:\n");
            prose.push_str("```json\n");
            prose.push_str(&formatted_settings);
            prose.push_str("\n```\n\n");
        }
        Err(_) => {
            prose.push_str("Effective workflow settings: (unable to format)\n\n");
        }
    }
}

fn format_tasks_section(prose: &mut String, tasks: &[ExplainTask]) {
    prose.push_str("## Execution Steps\n\n");
    prose.push_str("Execute the following tasks according to their transition conditions. ");
    prose.push_str("Tasks are listed in dependency order, but actual execution depends on the transition logic.\n\n");

    for (index, task) in tasks.iter().enumerate() {
        format_single_task(prose, task, index + 1);
    }
}

fn format_single_task(prose: &mut String, task: &ExplainTask, task_number: usize) {
    prose.push_str(&format!(
        "### {}: {} ({})\n\n",
        task_number, task.id, task.operator
    ));

    format_task_parameters(prose, &task.params);
    format_task_transitions(prose, &task.transitions);
}

fn format_task_parameters(prose: &mut String, params: &Value) {
    prose.push_str("**Parameters:**\n");
    match serde_json::to_string_pretty(params) {
        Ok(formatted_params) => {
            let formatted_params = format_runtime_placeholders(&formatted_params);
            prose.push_str("```json\n");
            prose.push_str(&formatted_params);
            prose.push_str("\n```\n\n");
        }
        Err(_) => {
            prose.push_str("(unable to format parameters)\n\n");
        }
    }
}

fn format_task_transitions(prose: &mut String, transitions: &[ExplainTransition]) {
    if !transitions.is_empty() {
        prose.push_str("**Transitions after completion:**\n");
        for transition in transitions {
            prose.push_str(&format!(
                "- Go to task '{}' with priority {} when: {}\n",
                transition.target, transition.priority, transition.when
            ));
        }
        prose.push('\n');
    } else {
        prose.push_str("**Transitions:** None (terminal task)\n\n");
    }
}

fn format_execution_notes(prose: &mut String) {
    prose.push_str("## Execution Notes\n\n");
    prose.push_str(
        "- Parameters marked as \"(runtime)\" will be provided or calculated during execution\n",
    );
    prose.push_str("- Transition conditions should be evaluated after each task completes\n");
    prose
        .push_str("- Execute transitions in priority order (lower numbers have higher priority)\n");
    prose.push_str("- If no transition conditions match, the workflow terminates\n");
    prose.push_str(
        "- Tasks without transitions are terminal tasks that end the workflow when completed\n",
    );
}

fn format_runtime_placeholders(json_str: &str) -> String {
    json_str.replace(
        &format!("\"{RUNTIME_PLACEHOLDER}\""),
        &format!("\"{RUNTIME_PLACEHOLDER}\" (value provided at runtime)"),
    )
}
