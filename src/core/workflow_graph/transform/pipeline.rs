use crate::core::error::AppError;
use crate::core::workflow_graph::schema::WorkflowDocument;
use crate::core::workflow_graph::transform::include_if::{
    ExprPrecompileTransform, IncludeIfTransform, NormalizeSchemaTransform,
};
use crate::core::workflow_graph::transform::macros::MacroExpansionTransform;
use crate::core::workflow_graph::transform::template::TemplateStringTransform;
use crate::core::workflow_graph::transform::WorkflowTransform;

pub fn apply_default_pipeline(doc: WorkflowDocument) -> Result<WorkflowDocument, AppError> {
    let transforms: Vec<Box<dyn WorkflowTransform>> = vec![
        Box::new(NormalizeSchemaTransform),
        Box::new(MacroExpansionTransform),
        Box::new(IncludeIfTransform),
        Box::new(TemplateStringTransform),
        Box::new(ExprPrecompileTransform),
    ];
    let mut current = doc;
    for transform in transforms {
        current = transform.transform(current)?;
    }
    Ok(current)
}
