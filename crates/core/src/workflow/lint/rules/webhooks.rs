use super::super::{LintResult, LintSeverity, WorkflowLintRule};
use crate::workflow::schema::WorkflowDocument;

struct RequiredTriggersRule;

impl WorkflowLintRule for RequiredTriggersRule {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        if workflow.workflow.settings.required_triggers.is_empty() {
            return Vec::new();
        }
        if workflow.triggers.is_some() {
            return Vec::new();
        }
        vec![LintResult::new(
            "WFG-LINT-009",
            LintSeverity::Warning,
            "required_triggers is set but workflow has no triggers block",
            None,
            Some("add a triggers block or provide trigger payloads at run time".to_string()),
        )]
    }
}

pub(super) fn rules() -> Vec<Box<dyn WorkflowLintRule>> {
    vec![Box::new(RequiredTriggersRule)]
}
