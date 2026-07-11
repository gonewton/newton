use crate::core::error::AppError;
use crate::workflow::schema::WorkflowDocument;
use crate::workflow::transform::include_if::{
    ExprPrecompileTransform, IncludeIfTransform, NormalizeSchemaTransform,
};
use crate::workflow::transform::macros::MacroExpansionTransform;
use crate::workflow::transform::template::TemplateStringTransform;
use crate::workflow::transform::WorkflowTransform;

/// Run the standard transform pipeline over a freshly parsed workflow document.
///
/// `allow_env_fn` gates the Rhai `env()` expression function (spec 074 S8) for
/// the transforms that actually *evaluate* expressions at transform time
/// (macro `with:` args, `include_if` conditions, `{{ }}` template strings) —
/// callers driving a real execution should pass the document's own
/// `workflow.settings.allow_env_fn`; callers that only need to confirm a
/// workflow parses/lints (validate, explain, lint, preview-on-save) should
/// pass `false` to stay deterministic. `ExprPrecompileTransform` only
/// compiles (parses) `$expr` strings — Rhai does not resolve function names
/// at parse time — so it never needs the flag.
pub fn apply_default_pipeline(
    doc: WorkflowDocument,
    allow_env_fn: bool,
) -> Result<WorkflowDocument, AppError> {
    let transforms: Vec<Box<dyn WorkflowTransform>> = vec![
        Box::new(NormalizeSchemaTransform),
        Box::new(MacroExpansionTransform::new(allow_env_fn)),
        Box::new(IncludeIfTransform::new(allow_env_fn)),
        Box::new(TemplateStringTransform::new(allow_env_fn)),
        Box::new(ExprPrecompileTransform),
    ];
    let mut current = doc;
    for transform in transforms {
        current = transform.transform(current)?;
    }
    Ok(current)
}
