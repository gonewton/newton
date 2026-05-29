use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use async_trait::async_trait;
use std::process::Stdio;
use tokio::process::Command;

#[derive(Clone, Debug)]
pub struct GhOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[async_trait]
pub trait GhRunner: Send + Sync + 'static {
    async fn run(&self, args: &[&str], cwd: &std::path::Path) -> Result<GhOutput, AppError>;
}

#[async_trait]
pub trait GitRunner: Send + Sync + 'static {
    async fn run(&self, args: &[&str], cwd: &std::path::Path) -> Result<GhOutput, AppError>;
}

pub struct TokioGhRunner;

pub fn default_runner() -> TokioGhRunner {
    TokioGhRunner
}

pub struct TokioGitRunner;

pub fn default_git_runner() -> TokioGitRunner {
    TokioGitRunner
}

async fn run_subcommand(
    binary: &str,
    spawn_error_code: &str,
    exit_error_code: &str,
    args: &[&str],
    cwd: &std::path::Path,
) -> Result<GhOutput, AppError> {
    let mut cmd = Command::new(binary);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    let output = cmd.output().await.map_err(|e| {
        AppError::new(
            ErrorCategory::ToolExecutionError,
            format!("failed to execute {binary}: {e}"),
        )
        .with_code(spawn_error_code)
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let exit_code = output.status.code().unwrap_or(-1);

    if exit_code != 0 {
        return Err(AppError::new(
            ErrorCategory::ToolExecutionError,
            format!("{binary} command failed with exit code {exit_code}: {stderr}"),
        )
        .with_code(exit_error_code));
    }

    Ok(GhOutput {
        stdout,
        stderr,
        exit_code,
    })
}

#[async_trait]
impl GhRunner for TokioGhRunner {
    async fn run(&self, args: &[&str], cwd: &std::path::Path) -> Result<GhOutput, AppError> {
        run_subcommand("gh", "WFG-GH-003", "WFG-GH-004", args, cwd).await
    }
}

#[async_trait]
impl GitRunner for TokioGitRunner {
    async fn run(&self, args: &[&str], cwd: &std::path::Path) -> Result<GhOutput, AppError> {
        run_subcommand("git", "WFG-GH-010", "WFG-GH-011", args, cwd).await
    }
}
