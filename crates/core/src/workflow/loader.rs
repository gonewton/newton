use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::expression::ExpressionEngine;
use crate::workflow::lint::{LintRegistry, LintResult, LintSeverity};
use crate::workflow::schema::WorkflowDocument;
use crate::workflow::transform;
use std::path::Path;

pub fn load_and_lint_workflow(
    workflow_path: &Path,
) -> Result<(WorkflowDocument, Vec<LintResult>), AppError> {
    let raw_document = crate::workflow::schema::parse_workflow(workflow_path)?;
    let document = transform::apply_default_pipeline(raw_document)?;
    let lint_results = LintRegistry::new().run(&document);
    check_lint_errors(&lint_results)?;
    Ok((document, lint_results))
}

pub fn check_lint_errors(lint_results: &[LintResult]) -> Result<(), AppError> {
    let error_count = lint_results
        .iter()
        .filter(|r| r.severity == LintSeverity::Error)
        .count();
    if error_count > 0 {
        Err(AppError::new(
            ErrorCategory::ValidationError,
            format!("workflow lint detected {error_count} error(s); fix before running"),
        ))
    } else {
        Ok(())
    }
}

pub fn check_lint_errors_after_run(document: &WorkflowDocument) -> Result<(), AppError> {
    let lint_results = LintRegistry::new().run(document);
    check_lint_errors(&lint_results)?;
    document.validate(&ExpressionEngine::default())?;
    Ok(())
}
