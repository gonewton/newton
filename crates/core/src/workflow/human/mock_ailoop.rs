//! Test double interviewer that mimics ailoop semantics.
//!
//! Returns scripted `ApprovalResult` / `DecisionResult` values from FIFO queues.
//! Used by HIL tests in place of `ConsoleInterviewer`. Reports
//! `interviewer_type() == "mock_ailoop"`.

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::human::{
    ApprovalDefault, ApprovalResult, DecisionContent, DecisionResult, Interviewer,
};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

pub struct MockAiloopInterviewer {
    approvals: Mutex<VecDeque<ApprovalResult>>,
    choices: Mutex<VecDeque<DecisionResult>>,
    decisions: Mutex<VecDeque<DecisionResult>>,
}

impl MockAiloopInterviewer {
    pub fn new() -> Self {
        Self {
            approvals: Mutex::new(VecDeque::new()),
            choices: Mutex::new(VecDeque::new()),
            decisions: Mutex::new(VecDeque::new()),
        }
    }

    pub fn push_approval(&self, result: ApprovalResult) {
        self.approvals.lock().unwrap().push_back(result);
    }

    pub fn push_choice(&self, result: DecisionResult) {
        self.choices.lock().unwrap().push_back(result);
    }

    pub fn push_decision(&self, result: DecisionResult) {
        self.decisions.lock().unwrap().push_back(result);
    }
}

impl Default for MockAiloopInterviewer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Interviewer for MockAiloopInterviewer {
    fn interviewer_type(&self) -> &'static str {
        "mock_ailoop"
    }

    async fn ask_approval(
        &self,
        _prompt: &str,
        _timeout: Option<Duration>,
        _default_on_timeout: Option<ApprovalDefault>,
    ) -> Result<ApprovalResult, AppError> {
        self.approvals.lock().unwrap().pop_front().ok_or_else(|| {
            AppError::new(
                ErrorCategory::InternalError,
                "MockAiloopInterviewer: no scripted approval available",
            )
        })
    }

    async fn ask_choice(
        &self,
        _prompt: &str,
        _choices: &[String],
        _timeout: Option<Duration>,
        _default_choice: Option<&str>,
    ) -> Result<DecisionResult, AppError> {
        self.choices.lock().unwrap().pop_front().ok_or_else(|| {
            AppError::new(
                ErrorCategory::InternalError,
                "MockAiloopInterviewer: no scripted choice available",
            )
        })
    }

    async fn ask_decision(
        &self,
        _content: DecisionContent,
        _timeout: Option<Duration>,
        _default_choice: Option<&str>,
    ) -> Result<DecisionResult, AppError> {
        self.decisions.lock().unwrap().pop_front().ok_or_else(|| {
            AppError::new(
                ErrorCategory::InternalError,
                "MockAiloopInterviewer: no scripted decision available",
            )
        })
    }
}
