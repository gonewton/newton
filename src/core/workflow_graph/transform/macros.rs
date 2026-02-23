use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::expression::{EvaluationContext, ExpressionEngine};
use crate::core::workflow_graph::schema::{TaskOrMacro, WorkflowDocument, WorkflowTask};
use crate::core::workflow_graph::transform::WorkflowTransform;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub struct MacroExpansionTransform;

impl WorkflowTransform for MacroExpansionTransform {
    fn name(&self) -> &'static str {
        "MacroExpansionTransform"
    }

    fn transform(&self, doc: WorkflowDocument) -> Result<WorkflowDocument, AppError> {
        let mut doc = doc;
        let definitions = doc.macros.clone().unwrap_or_default();
        let macros_by_name: HashMap<String, Vec<WorkflowTask>> = definitions
            .into_iter()
            .map(|def| (def.name, def.tasks))
            .collect();

        let engine = ExpressionEngine::default();
        let mut expanded: Vec<TaskOrMacro> = Vec::new();
        for item in doc.workflow.tasks {
            match item {
                TaskOrMacro::Task(task) => expanded.push(TaskOrMacro::Task(task)),
                TaskOrMacro::Macro(invocation) => {
                    let macro_tasks =
                        macros_by_name.get(&invocation.macro_name).ok_or_else(|| {
                            AppError::new(
                                ErrorCategory::ValidationError,
                                format!("unknown macro invocation '{}'", invocation.macro_name),
                            )
                            .with_code("WFG-MACRO-002")
                        })?;
                    let local_scope = Value::Object(invocation.with.clone());
                    let ctx = EvaluationContext::new(
                        local_scope,
                        Value::Object(serde_json::Map::new()),
                        Value::Object(serde_json::Map::new()),
                    );
                    for task in macro_tasks {
                        let transformed = interpolate_task(task, &engine, &ctx)?;
                        expanded.push(TaskOrMacro::Task(transformed));
                    }
                }
            }
        }

        let mut seen = HashSet::new();
        for task in expanded.iter().filter_map(TaskOrMacro::as_task) {
            if !seen.insert(task.id.as_str()) {
                return Err(AppError::new(
                    ErrorCategory::ValidationError,
                    format!("macro expansion produced duplicate task id '{}'", task.id),
                )
                .with_code("WFG-MACRO-001"));
            }
        }

        doc.workflow.tasks = expanded;
        Ok(doc)
    }
}

fn interpolate_task(
    task: &WorkflowTask,
    engine: &ExpressionEngine,
    ctx: &EvaluationContext,
) -> Result<WorkflowTask, AppError> {
    let mut serialized = serde_json::to_value(task).map_err(|err| {
        AppError::new(
            ErrorCategory::SerializationError,
            format!("failed to serialize macro task template: {}", err),
        )
    })?;
    interpolate_value(&mut serialized, engine, ctx, "macro_task")?;
    serde_json::from_value(serialized).map_err(|err| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("failed to deserialize expanded macro task: {}", err),
        )
    })
}

fn interpolate_value(
    value: &mut Value,
    engine: &ExpressionEngine,
    ctx: &EvaluationContext,
    field: &str,
) -> Result<(), AppError> {
    match value {
        Value::String(text) => {
            *text = engine.interpolate_string(text, ctx).map_err(|err| {
                AppError::new(
                    ErrorCategory::ValidationError,
                    format!(
                        "template interpolation error in '{}': {}",
                        field, err.message
                    ),
                )
                .with_code("WFG-TPL-001")
            })?;
        }
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                let location = format!("{}[{}]", field, index);
                interpolate_value(item, engine, ctx, &location)?;
            }
        }
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                let location = format!("{}.{}", field, key);
                interpolate_value(item, engine, ctx, &location)?;
            }
        }
        _ => {}
    }
    Ok(())
}
