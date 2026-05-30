use super::super::{LintResult, LintSeverity, WorkflowLintRule};
use crate::workflow::expression::{EvaluationContext, ExpressionEngine};
use crate::workflow::schema::{Condition, WorkflowDocument};
use serde_json::{Map, Value};
use std::collections::HashSet;

struct ExpressionParseFailureRule;

impl WorkflowLintRule for ExpressionParseFailureRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let engine = ExpressionEngine::default();
        let mut exprs = Vec::new();
        collect_expr_values(&workflow.workflow.context, &mut exprs, None);
        for task in workflow.workflow.tasks() {
            collect_expr_values(&task.params, &mut exprs, Some(task.id.as_str()));
            for transition in &task.transitions {
                if let Some(Condition::Expr { expr }) = &transition.when {
                    exprs.push((expr.clone(), Some(task.id.clone())));
                }
            }
        }

        let mut out = Vec::new();
        for (expr, location) in exprs {
            if let Err(err) = engine.compile(&expr) {
                out.push(LintResult::new(
                    "WFG-LINT-005",
                    LintSeverity::Error,
                    format!("$expr parse failure for '{}': {}", expr, err.message),
                    location,
                    Some("fix syntax so the expression compiles".to_string()),
                ));
            }
        }
        out
    }
}

struct WhenExpressionBoolRule;

impl WorkflowLintRule for WhenExpressionBoolRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let engine = ExpressionEngine::default();
        let mut out = Vec::new();

        for task in workflow.workflow.tasks() {
            for transition in &task.transitions {
                let Some(Condition::Expr { expr }) = &transition.when else {
                    continue;
                };
                if expr_depends_on_tasks(expr) {
                    continue;
                }

                let eval_ctx = EvaluationContext::new(
                    workflow.workflow.context.clone(),
                    Value::Object(Map::new()),
                    Value::Object(Map::new()),
                );

                match engine.evaluate(expr, &eval_ctx) {
                    Ok(Value::Bool(_)) => {}
                    Ok(_) => out.push(LintResult::new(
                        "WFG-LINT-006",
                        LintSeverity::Error,
                        format!(
                            "$expr in transition 'when' for task '{}' does not evaluate to bool",
                            task.id
                        ),
                        Some(task.id.clone()),
                        Some("ensure transition 'when' expressions return true/false".to_string()),
                    )),
                    Err(_) => {}
                }
            }
        }

        out
    }
}

struct StaticTaskIdContainsColonRule;

impl WorkflowLintRule for StaticTaskIdContainsColonRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let mut out = Vec::new();
        for task in workflow.workflow.tasks() {
            if task.id.contains(':') {
                out.push(LintResult::new(
                    "WFG-LINT-119",
                    LintSeverity::Error,
                    format!(
                        "Static task ID '{}' contains colon which is reserved for dynamic namespacing",
                        task.id
                    ),
                    Some(task.id.clone()),
                    Some("Remove colon from task ID or use a different character".to_string()),
                ));
            }
        }
        out
    }
}

struct IoResultMapTaskRefsRule;

impl WorkflowLintRule for IoResultMapTaskRefsRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let result_map = match &workflow.workflow.settings.io.result_map {
            None => return vec![],
            Some(rm) => rm,
        };
        let known_ids: HashSet<&str> = workflow
            .workflow
            .tasks()
            .map(|task| task.id.as_str())
            .collect();
        let mut out = Vec::new();
        for (key, value) in result_map {
            if let Value::String(s) = value {
                if let Some(expr) = s.strip_prefix("$expr:") {
                    let mut remaining = expr;
                    while let Some(pos) = remaining.find("tasks['") {
                        let after = &remaining[pos + 7..];
                        if let Some(end) = after.find("']") {
                            let task_ref = &after[..end];
                            if !known_ids.contains(task_ref) {
                                out.push(LintResult::new(
                                    "WFG-LINT-120",
                                    LintSeverity::Warning,
                                    format!("result_map key '{key}' references undeclared task '{task_ref}'"),
                                    Some(format!("io.result_map.{key}")),
                                    Some("update result_map to reference only declared task ids".to_string()),
                                ));
                            }
                            remaining = &after[end + 2..];
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        out
    }
}

struct IoSchemaTypeRule;

impl WorkflowLintRule for IoSchemaTypeRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let io = &workflow.workflow.settings.io;
        let mut out = Vec::new();
        for (field, schema) in [
            ("io.input_schema", &io.input_schema),
            ("io.output_schema", &io.output_schema),
        ] {
            if let Some(schema_val) = schema {
                let type_ok = schema_val
                    .as_object()
                    .and_then(|m| m.get("type"))
                    .and_then(Value::as_str)
                    .map(|t| t == "object")
                    .unwrap_or(false);
                if !type_ok {
                    out.push(LintResult::new(
                        "WFG-LINT-121",
                        LintSeverity::Error,
                        format!("{field} must have top-level type: object"),
                        Some(field.to_string()),
                        Some("add `type: object` to the schema root".to_string()),
                    ));
                }
            }
        }
        out
    }
}

struct IoOutputSchemaRequiresResultMapRule;

impl WorkflowLintRule for IoOutputSchemaRequiresResultMapRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let io = &workflow.workflow.settings.io;
        if io.output_schema.is_some() && io.result_map.is_none() {
            return vec![LintResult::new(
                "WFG-LINT-122",
                LintSeverity::Error,
                "io.output_schema is defined but io.result_map is absent",
                Some("io.output_schema".to_string()),
                Some(
                    "add result_map to produce the output that output_schema validates".to_string(),
                ),
            )];
        }
        vec![]
    }
}

fn expr_depends_on_tasks(expr: &str) -> bool {
    expr.contains("tasks.") || expr.contains("tasks[")
}

fn collect_expr_values(
    value: &Value,
    out: &mut Vec<(String, Option<String>)>,
    location: Option<&str>,
) {
    match value {
        Value::Object(map) => {
            if map.len() == 1 && map.contains_key("$expr") {
                if let Some(Value::String(expr)) = map.get("$expr") {
                    out.push((expr.clone(), location.map(ToOwned::to_owned)));
                    return;
                }
            }
            for child in map.values() {
                collect_expr_values(child, out, location);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_expr_values(child, out, location);
            }
        }
        _ => {}
    }
}

pub(super) fn rules() -> Vec<Box<dyn WorkflowLintRule>> {
    vec![
        Box::new(ExpressionParseFailureRule),
        Box::new(WhenExpressionBoolRule),
        Box::new(StaticTaskIdContainsColonRule),
        Box::new(IoResultMapTaskRefsRule),
        Box::new(IoSchemaTypeRule),
        Box::new(IoOutputSchemaRequiresResultMapRule),
    ]
}
