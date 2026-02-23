#![allow(clippy::result_large_err)] // Explain returns AppError to preserve structured evaluation diagnostics.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::expression::{EvaluationContext, ExpressionEngine};
use crate::core::workflow_graph::schema::{Condition, WorkflowDocument, WorkflowTask};
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
            format!("failed to serialize workflow settings: {}", err),
        )
    })?;
    let engine = ExpressionEngine::default();
    let mut diagnostics = Vec::new();

    let tasks = document
        .workflow
        .tasks
        .iter()
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
