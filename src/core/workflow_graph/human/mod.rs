use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::str::FromStr;
use std::time::Duration;

/// Default outcome applied when an approval times out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDefault {
    Approve,
    Reject,
}

impl ApprovalDefault {
    /// Return a lowercase string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalDefault::Approve => "approve",
            ApprovalDefault::Reject => "reject",
        }
    }
}

impl FromStr for ApprovalDefault {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_lowercase().as_str() {
            "approve" => Ok(ApprovalDefault::Approve),
            "reject" => Ok(ApprovalDefault::Reject),
            _ => Err("must be 'approve' or 'reject'"),
        }
    }
}

impl std::fmt::Display for ApprovalDefault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Result returned from an approval prompt.
#[derive(Debug, Clone)]
pub struct ApprovalResult {
    pub approved: bool,
    pub reason: String,
    pub timestamp: DateTime<Utc>,
    pub timeout_applied: bool,
    pub default_used: bool,
}

impl ApprovalResult {
    pub fn with_defaults(approved: bool, reason: String) -> Self {
        Self {
            approved,
            reason,
            timestamp: Utc::now(),
            timeout_applied: false,
            default_used: false,
        }
    }
}

/// Result returned from a multi-choice prompt.
#[derive(Debug, Clone)]
pub struct DecisionResult {
    pub choice: String,
    pub timestamp: DateTime<Utc>,
    pub timeout_applied: bool,
    pub default_used: bool,
    pub response_text: Option<String>,
}

/// Interface for blocking human input flows within workflow operators.
#[async_trait]
pub trait Interviewer: Send + Sync + 'static {
    /// Human-friendly identifier used in audit logs.
    fn interviewer_type(&self) -> &'static str;

    async fn ask_approval(
        &self,
        prompt: &str,
        timeout: Option<Duration>,
        default_on_timeout: Option<ApprovalDefault>,
    ) -> Result<ApprovalResult, crate::core::error::AppError>;

    async fn ask_choice(
        &self,
        prompt: &str,
        choices: &[String],
        timeout: Option<Duration>,
        default_choice: Option<&str>,
    ) -> Result<DecisionResult, crate::core::error::AppError>;
}

pub mod audit;
pub mod console;

pub use audit::AuditEntry;
pub use console::ConsoleInterviewer;
