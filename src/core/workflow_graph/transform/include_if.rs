use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::expression::{EvaluationContext, ExpressionEngine};
use crate::core::workflow_graph::schema::{Condition, TaskOrMacro, WorkflowDocument};
use crate::core::workflow_graph::transform::WorkflowTransform;
use serde_json::{Map, Value};
use std::collections::HashSet;

pub struct NormalizeSchemaTransform;
pub struct IncludeIfTransform;
pub struct ExprPrecompileTransform;

impl WorkflowTransform for NormalizeSchemaTransform {
    fn name(&self) -> &'static str {
        "NormalizeSchemaTransform"
    }

    fn transform(&self, doc: WorkflowDocument) -> Result<WorkflowDocument, AppError> {
        Ok(doc)
    }
}

impl WorkflowTransform for IncludeIfTransform {
    fn name(&self) -> &'static str {
        "IncludeIfTransform"
    }

    fn transform(&self, doc: WorkflowDocument) -> Result<WorkflowDocument, AppError> {
        let mut doc = doc;
        let engine = ExpressionEngine::default();
        let triggers = doc
            .triggers
            .as_ref()
            .map(|trigger| trigger.payload.clone())
            .unwrap_or_else(|| Value::Object(Map::new()));
        let context = doc.workflow.context.clone();
        let eval_ctx = EvaluationContext::new(context, Value::Object(Map::new()), triggers);

        let mut retained = Vec::new();
        let mut removed_task_ids = HashSet::new();
        for item in doc.workflow.tasks {
            match item {
                TaskOrMacro::Macro(invocation) => retained.push(TaskOrMacro::Macro(invocation)),
                TaskOrMacro::Task(mut task) => {
                    let include = evaluate_include_if(
                        task.include_if.as_ref(),
                        &task.id,
                        "task",
                        &eval_ctx,
                        &engine,
                    )?;
                    if include {
                        task.include_if = None;
                        retained.push(TaskOrMacro::Task(task));
                    } else {
                        removed_task_ids.insert(task.id);
                    }
                }
            }
        }

        for item in &mut retained {
            if let Some(task) = item.as_task_mut() {
                let mut kept = Vec::new();
                for mut transition in task.transitions.clone() {
                    if removed_task_ids.contains(&transition.to) {
                        continue;
                    }
                    let include = evaluate_include_if(
                        transition.include_if.as_ref(),
                        &task.id,
                        "transition",
                        &eval_ctx,
                        &engine,
                    )?;
                    transition.include_if = None;
                    if include {
                        kept.push(transition);
                    }
                }
                task.transitions = kept;
            }
        }
        doc.workflow.tasks = retained;
        Ok(doc)
    }
}

impl WorkflowTransform for ExprPrecompileTransform {
    fn name(&self) -> &'static str {
        "ExprPrecompileTransform"
    }

    fn transform(&self, doc: WorkflowDocument) -> Result<WorkflowDocument, AppError> {
        let engine = ExpressionEngine::default();
        let mut expressions = Vec::new();
        collect_expression_strings(&doc.workflow.context, &mut expressions);
        for task in doc.workflow.tasks() {
            collect_expression_strings(&task.params, &mut expressions);
            if let Some(include_if) = &task.include_if {
                if let Some(expr) = include_if.expression() {
                    expressions.push(expr.to_string());
                }
            }
            for transition in &task.transitions {
                if let Some(include_if) = &transition.include_if {
                    if let Some(expr) = include_if.expression() {
                        expressions.push(expr.to_string());
                    }
                }
                if let Some(when) = &transition.when {
                    if let Some(expr) = when.expression() {
                        expressions.push(expr.to_string());
                    }
                }
            }
        }
        for expr in expressions {
            engine.compile(&expr).map_err(|err| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    format!("$expr parse failure for '{}': {}", expr, err.message),
                )
                .with_code("WFG-LINT-005")
            })?;
        }
        Ok(doc)
    }
}

fn evaluate_include_if(
    condition: Option<&Condition>,
    task_id: &str,
    field: &str,
    eval_ctx: &EvaluationContext,
    engine: &ExpressionEngine,
) -> Result<bool, AppError> {
    let Some(condition) = condition else {
        return Ok(true);
    };
    match condition {
        Condition::Bool(flag) => Ok(*flag),
        Condition::Expr { expr } => {
            if expr.contains("tasks") {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "include_if may not reference `tasks` — task results are not available at transform time (location: '{}')",
                        task_id
                    ),
                )
                .with_code("WFG-INCLUDE-001"));
            }
            let evaluated = engine.evaluate(expr, eval_ctx).map_err(|err| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "template interpolation error in '{}.{}': {}",
                        task_id, field, err
                    ),
                )
                .with_code("WFG-TPL-001")
            })?;
            Ok(is_truthy(&evaluated))
        }
    }
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(i) = number.as_i64() {
                i != 0
            } else if let Some(u) = number.as_u64() {
                u != 0
            } else if let Some(f) = number.as_f64() {
                f != 0.0
            } else {
                false
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn collect_expression_strings(value: &Value, expressions: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if map.len() == 1 && map.contains_key("$expr") {
                if let Some(Value::String(expr)) = map.get("$expr") {
                    expressions.push(expr.clone());
                    return;
                }
            }
            for child in map.values() {
                collect_expression_strings(child, expressions);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_expression_strings(item, expressions);
            }
        }
        _ => {}
    }
}
