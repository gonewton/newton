//! Strict toolchain operations are not implemented yet; each runner surfaces an explicit error instead of silently succeeding.

use crate::{core::error::AppError, core::types::ErrorCategory, Result};

pub struct StrictToolchainRunner;

impl StrictToolchainRunner {
    pub fn new() -> Self {
        StrictToolchainRunner
    }

    pub async fn run_evaluator(&self, _workspace_path: &str) -> Result<()> {
        Err(AppError::new(
            ErrorCategory::ToolExecutionError,
            "strict toolchain evaluator mode is not available yet",
        )
        .into())
    }

    pub async fn run_advisor(&self, _workspace_path: &str) -> Result<()> {
        Err(AppError::new(
            ErrorCategory::ToolExecutionError,
            "strict toolchain advisor mode is not available yet",
        )
        .into())
    }

    pub async fn run_executor(&self, _workspace_path: &str) -> Result<()> {
        Err(AppError::new(
            ErrorCategory::ToolExecutionError,
            "strict toolchain executor mode is not available yet",
        )
        .into())
    }
}

impl Default for StrictToolchainRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn evaluator_returns_error() {
        let runner = StrictToolchainRunner::new();
        let err = runner.run_evaluator("/tmp").await.unwrap_err();
        assert!(
            err.to_string().contains("strict toolchain evaluator"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn advisor_returns_error() {
        let runner = StrictToolchainRunner::new();
        let err = runner.run_advisor("/tmp").await.unwrap_err();
        assert!(
            err.to_string().contains("strict toolchain advisor"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn executor_returns_error() {
        let runner = StrictToolchainRunner::new();
        let err = runner.run_executor("/tmp").await.unwrap_err();
        assert!(
            err.to_string().contains("strict toolchain executor"),
            "unexpected error: {err}"
        );
    }
}
