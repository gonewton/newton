#![allow(clippy::result_large_err)] // Lint module surfaces rich diagnostics via AppError without boxing.

use crate::core::workflow_graph::schema::WorkflowDocument;
use serde::Serialize;
use std::cmp::Ordering;
use std::fmt;

mod rules;

pub use rules::built_in_rules;

/// Lint severity for workflow diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LintSeverity {
    Error,
    Warning,
    Info,
}

impl LintSeverity {
    fn rank(self) -> u8 {
        match self {
            LintSeverity::Error => 3,
            LintSeverity::Warning => 2,
            LintSeverity::Info => 1,
        }
    }
}

impl fmt::Display for LintSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            LintSeverity::Error => "error",
            LintSeverity::Warning => "warning",
            LintSeverity::Info => "info",
        };
        write!(f, "{}", value)
    }
}

/// A single lint finding for a workflow document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LintResult {
    pub code: String,
    pub severity: LintSeverity,
    pub message: String,
    pub location: Option<String>,
    pub suggestion: Option<String>,
}

impl LintResult {
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

/// A lint rule that validates a workflow and returns zero or more findings.
pub trait WorkflowLintRule: Send + Sync {
    fn validate(&self, workflow: &WorkflowDocument) -> Vec<LintResult>;
}

/// Registry for built-in workflow lint rules.
pub struct LintRegistry {
    rules: Vec<Box<dyn WorkflowLintRule>>,
}

impl LintRegistry {
    pub fn new() -> Self {
        Self {
            rules: built_in_rules(),
        }
    }

    pub fn run(&self, workflow: &WorkflowDocument) -> Vec<LintResult> {
        let mut results = Vec::new();
        for rule in &self.rules {
            results.extend(rule.validate(workflow));
        }
        sort_results(&mut results);
        results
    }
}

impl Default for LintRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn sort_results(results: &mut [LintResult]) {
    results.sort_by(compare_result);
}

fn compare_result(a: &LintResult, b: &LintResult) -> Ordering {
    b.severity
        .rank()
        .cmp(&a.severity.rank())
        .then_with(|| a.code.cmp(&b.code))
        .then_with(|| a.location.cmp(&b.location))
}
