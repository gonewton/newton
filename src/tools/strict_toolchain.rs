use crate::Result;

pub struct StrictToolchainRunner;

impl StrictToolchainRunner {
    pub fn new() -> Self {
        StrictToolchainRunner
    }

    pub async fn run_evaluator(&self, _workspace_path: &str) -> Result<()> {
        // TODO: Implement strict toolchain evaluator
        Ok(())
    }

    pub async fn run_advisor(&self, _workspace_path: &str) -> Result<()> {
        // TODO: Implement strict toolchain advisor
        Ok(())
    }

    pub async fn run_executor(&self, _workspace_path: &str) -> Result<()> {
        // TODO: Implement strict toolchain executor
        Ok(())
    }
}