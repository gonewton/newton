use crate::core::workflow_graph::expression::{EvaluationContext, ExpressionEngine};
use crate::core::workflow_graph::schema::{Condition, WorkflowDocument, WorkflowTask};
use serde::Serialize;
use serde_json::{Map, Value};

/// Output produced by `newton workflow explain`.
#[derive(Debug, Clone, Serialize)]
pub struct ExplainOutput {
    pub settings: crate::core::workflow_graph::schema::WorkflowSettings,
    pub context: Value,
    pub triggers: Value,
    pub tasks: Vec<TaskExplain>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskExplain {
    pub id: String,
    pub operator: String,
    pub params: Value,
    pub transitions: Vec<TransitionExplain>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransitionExplain {
    pub target: String,
    pub priority: i32,
    pub when: String,
}

/// Build the explainability snapshot for the provided workflow document.
pub fn build_explain_output(
    document: &WorkflowDocument,
    context_overrides: &[(String, Value)],
) -> ExplainOutput {
    let mut context = document.workflow.context.clone();
    apply_context_overrides(&mut context, context_overrides);
    let engine = ExpressionEngine::default();
    let evaluation_context = evaluation_context_from_document(document, &context);

    let tasks = document
        .workflow
        .tasks
        .iter()
        .map(|task| explain_task(task, &engine, &evaluation_context))
        .collect();

    ExplainOutput {
        settings: document.workflow.settings.clone(),
        context,
        triggers: Value::Object(Map::new()),
        tasks,
    }
}

fn explain_task(
    task: &WorkflowTask,
    engine: &ExpressionEngine,
    ctx: &EvaluationContext,
) -> TaskExplain {
    let params = explain_value(&task.params, engine, ctx);
    let mut sorted_transitions = task.transitions.clone();
    sorted_transitions.sort_by_key(|transition| transition.priority);
    let transitions = sorted_transitions
        .into_iter()
        .map(|transition| TransitionExplain {
            target: transition.to.clone(),
            priority: transition.priority,
            when: format_condition(&transition.when),
        })
        .collect();

    TaskExplain {
        id: task.id.clone(),
        operator: task.operator.clone(),
        params,
        transitions,
    }
}

fn explain_value(value: &Value, engine: &ExpressionEngine, ctx: &EvaluationContext) -> Value {
    match value {
        Value::Object(map) if map.len() == 1 && map.contains_key("$expr") => {
            if let Some(Value::String(expr)) = map.get("$expr") {
                if expression_depends_on_tasks(expr) {
                    return Value::String("(runtime)".to_string());
                }
                if let Ok(evaluated) = engine.evaluate(expr, ctx) {
                    return evaluated;
                }
            }
            Value::Object(map.clone())
        }
        Value::Object(map) => {
            let mut resolved = Map::new();
            for (key, child) in map {
                resolved.insert(key.clone(), explain_value(child, engine, ctx));
            }
            Value::Object(resolved)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| explain_value(item, engine, ctx))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn format_condition(condition: &Option<Condition>) -> String {
    match condition {
        None => "(always)".to_string(),
        Some(Condition::Bool(flag)) => flag.to_string(),
        Some(Condition::Expr { expr }) => expr.clone(),
    }
}

fn apply_context_overrides(context: &mut Value, overrides: &[(String, Value)]) {
    if !context.is_object() {
        *context = Value::Object(Map::new());
    }
    if let Some(map) = context.as_object_mut() {
        for (key, value) in overrides {
            map.insert(key.clone(), value.clone());
        }
    }
}

fn evaluation_context_from_document(
    document: &WorkflowDocument,
    context: &Value,
) -> EvaluationContext {
    let tasks = build_tasks_placeholder(document);
    let triggers = Value::Object(Map::new());
    EvaluationContext::new(context.clone(), tasks, triggers)
}

fn build_tasks_placeholder(document: &WorkflowDocument) -> Value {
    let mut map = Map::new();
    for task in &document.workflow.tasks {
        map.insert(task.id.clone(), Value::Object(Map::new()));
    }
    Value::Object(map)
}

fn expression_depends_on_tasks(expr: &str) -> bool {
    expr.contains("tasks.") || expr.contains("tasks[")
}
