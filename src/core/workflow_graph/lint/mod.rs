use crate::core::workflow_graph::expression::ExpressionEngine;
use crate::core::workflow_graph::schema::WorkflowDocument;
use serde::Serialize;
use std::fmt;

pub mod rules;
pub use rules::*;

/// Diagnostic severity levels emitted by workflow lint rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LintSeverity {
    Error,
    Warning,
    Info,
}

impl LintSeverity {
    fn rank(&self) -> u8 {
        match self {
            LintSeverity::Error => 3,
            LintSeverity::Warning => 2,
            LintSeverity::Info => 1,
        }
    }
}

impl fmt::Display for LintSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LintSeverity::Error => write!(f, "Error"),
            LintSeverity::Warning => write!(f, "Warning"),
            LintSeverity::Info => write!(f, "Info"),
        }
    }
}

/// Individual lint/validation result emitted by a rule.
#[derive(Debug, Clone, Serialize)]
pub struct LintResult {
    pub code: String,
    pub severity: LintSeverity,
    pub message: String,
    pub location: Option<String>,
    pub suggestion: Option<String>,
}

impl LintResult {
    /// Create a new lint result with optional location and suggestion.
    pub fn new(
        code: impl Into<String>,
        severity: LintSeverity,
        message: impl Into<String>,
        location: Option<String>,
        suggestion: Option<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity,
            message: message.into(),
            location,
            suggestion,
        }
    }
}

/// Trait implemented by workflow lint rules.
pub trait WorkflowLintRule {
    fn validate(&self, workflow: &WorkflowDocument, engine: &ExpressionEngine) -> Vec<LintResult>;
}

/// Registry that runs all built-in workflow lint rules.
pub struct LintRegistry {
    engine: ExpressionEngine,
    rules: Vec<Box<dyn WorkflowLintRule>>,
}

impl LintRegistry {
    /// Construct a registry populated with the built-in rules.
    pub fn new() -> Self {
        let rules: Vec<Box<dyn WorkflowLintRule>> = vec![
            Box::new(DuplicateTaskIdsRule),
            Box::new(UnknownTransitionTargetsRule),
            Box::new(UnreachableTaskRule),
            Box::new(AssertCompletedRequireRule),
            Box::new(ExpressionParseRule),
            Box::new(WhenConditionBooleanRule),
            Box::new(SuspiciousLoopRule),
            Box::new(CommandOperatorShellRule),
        ];
        Self {
            engine: ExpressionEngine::default(),
            rules,
        }
    }

    /// Run all registered lint rules against the workflow document.
    /// The results are already sorted by `(severity desc, code asc, location asc)`.
    pub fn run(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let mut results = Vec::new();
        for rule in &self.rules {
            results.extend(rule.validate(workflow, &self.engine));
        }
        results.sort_by(|a, b| {
            let severity_cmp = b.severity.rank().cmp(&a.severity.rank());
            severity_cmp
                .then(a.code.cmp(&b.code))
                .then(a.location.cmp(&b.location))
        });
        results
    }
}

impl Default for LintRegistry {
    fn default() -> Self {
        Self::new()
    }
}
