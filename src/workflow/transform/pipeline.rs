use crate::core::error::AppError;
use crate::workflow::schema::WorkflowDocument;
use crate::workflow::transform::include_if::{
    ExprPrecompileTransform, IncludeIfTransform, NormalizeSchemaTransform,
};
use crate::workflow::transform::macros::MacroExpansionTransform;
use crate::workflow::transform::template::TemplateStringTransform;
use crate::workflow::transform::WorkflowTransform;

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
