use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::core::workflow_graph::expression::{EvaluationContext, ExpressionEngine};
use crate::core::workflow_graph::schema::WorkflowDocument;
use crate::core::workflow_graph::transform::WorkflowTransform;
use serde_json::{Map, Value};

pub struct TemplateStringTransform;

impl WorkflowTransform for TemplateStringTransform {
    fn name(&self) -> &'static str {
        "TemplateStringTransform"
    }

    fn transform(&self, doc: WorkflowDocument) -> Result<WorkflowDocument, AppError> {
        let mut doc = doc;
        let engine = ExpressionEngine::default();
        let triggers = doc
            .triggers
            .as_ref()
            .map(|trigger| trigger.payload.clone())
            .unwrap_or_else(|| Value::Object(Map::new()));

        let context_snapshot = doc.workflow.context.clone();
        let eval_ctx = EvaluationContext::new(
            context_snapshot,
            Value::Object(Map::new()),
            triggers.clone(),
        );
        interpolate_value(
            &mut doc.workflow.context,
            &engine,
            &eval_ctx,
            "workflow.context",
        )?;

        let context_for_tasks = doc.workflow.context.clone();
        for task in doc.workflow.tasks_mut() {
            let task_ctx = EvaluationContext::new(
                context_for_tasks.clone(),
                Value::Object(Map::new()),
                triggers.clone(),
            );
            let field = format!("task '{}'.params", task.id);
            interpolate_value(&mut task.params, &engine, &task_ctx, &field)?;
        }
        Ok(doc)
    }
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
                let field = format!("{}[{}]", field, index);
                interpolate_value(item, engine, ctx, &field)?;
            }
        }
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                let field = format!("{}.{}", field, key);
                interpolate_value(item, engine, ctx, &field)?;
            }
        }
        _ => {}
    }
    Ok(())
}
