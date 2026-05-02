#![allow(clippy::result_large_err)] // Transform pipeline returns AppError for structured diagnostics.

use crate::core::error::AppError;
use crate::workflow::schema::WorkflowDocument;

mod include_if;
mod macros;
mod pipeline;
mod template;

pub use pipeline::apply_default_pipeline;

/// Pure transform from parsed workflow YAML to normalized workflow document.
pub trait WorkflowTransform {
    fn name(&self) -> &'static str;
    fn transform(&self, doc: WorkflowDocument) -> Result<WorkflowDocument, AppError>;
}
