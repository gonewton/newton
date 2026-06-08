#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

// ─── Params ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum GitParams {
    CleanCheck {},
    SyncMain {},
    CreateBranch {
        name: String,
    },
    Stage {
        #[serde(default)]
        exclude: Vec<String>,
    },
    Commit {
        message: String,
        #[serde(default)]
        allow_empty: bool,
    },
    Push {
        #[serde(default = "default_remote")]
        remote: String,
        #[serde(default = "default_true")]
        set_upstream: bool,
        #[serde(default = "default_retry_count")]
        retry_count: u32,
        #[serde(default = "default_retry_delay_ms")]
        retry_delay_ms: u64,
    },
    Diff {
        #[serde(default = "default_base")]
        base: String,
        #[serde(default = "default_max_bytes")]
        max_bytes: u64,
    },
    CleanupMerge {},
}

fn default_remote() -> String {
    "origin".to_string()
}
fn default_true() -> bool {
    true
}
fn default_retry_count() -> u32 {
    3
}
fn default_retry_delay_ms() -> u64 {
    5000
}
fn default_base() -> String {
    "main".to_string()
}
fn default_max_bytes() -> u64 {
    262144
}

// ─── Output shapes ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum GitOutput {
    Status {
        ok: bool,
        message: String,
    },
    Branch {
        name: String,
    },
    Stage {
        has_staged: bool,
    },
    Commit {
        committed: bool,
        skipped: bool,
        precommit_failed: bool,
    },
    Diff {
        stat: String,
        patch: String,
        bytes: u64,
    },
}

// ─── Operator struct ──────────────────────────────────────────────────────────

pub struct GitOperator;

impl GitOperator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GitOperator {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Shell helpers ────────────────────────────────────────────────────────────

struct ShellOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

async fn run_git(args: &[&str], cwd: &Path) -> Result<ShellOutput, AppError> {
    let mut cmd = Command::new("git");
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
            format!("failed to spawn git: {e}"),
        )
        .with_code("WFG-GIT-001")
    })?;

    Ok(ShellOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

async fn run_git_ok(args: &[&str], cwd: &Path) -> Result<ShellOutput, AppError> {
    let out = run_git(args, cwd).await?;
    if out.exit_code != 0 {
        return Err(AppError::new(
            ErrorCategory::ToolExecutionError,
            format!(
                "git {} failed (exit {}): {}",
                args.join(" "),
                out.exit_code,
                out.stderr.trim()
            ),
        )
        .with_code("WFG-GIT-002"));
    }
    Ok(out)
}

// ─── Operation implementations ────────────────────────────────────────────────

async fn execute_clean_check(cwd: &Path) -> Result<Value, AppError> {
    let diff = run_git(&["diff", "--quiet"], cwd).await?;
    if diff.exit_code != 0 {
        return Ok(json!({ "ok": false, "message": "working tree has unstaged changes" }));
    }
    let cached = run_git(&["diff", "--cached", "--quiet"], cwd).await?;
    if cached.exit_code != 0 {
        return Ok(json!({ "ok": false, "message": "working tree has staged changes" }));
    }
    Ok(json!({ "ok": true, "message": "" }))
}

async fn execute_sync_main(cwd: &Path) -> Result<Value, AppError> {
    run_git_ok(&["fetch", "origin"], cwd).await?;
    run_git_ok(&["checkout", "main"], cwd).await?;
    run_git_ok(&["pull", "--rebase", "origin", "main"], cwd).await?;
    Ok(json!({ "ok": true, "message": "" }))
}

async fn execute_create_branch(name: &str, cwd: &Path) -> Result<Value, AppError> {
    run_git_ok(&["checkout", "-b", name], cwd).await?;
    Ok(json!({ "name": name }))
}

async fn execute_stage(exclude: &[String], cwd: &Path) -> Result<Value, AppError> {
    run_git_ok(&["add", "-A"], cwd).await?;

    for pattern in exclude {
        let listed = run_git(&["diff", "--cached", "--name-only"], cwd).await?;
        let matching: Vec<String> = listed
            .stdout
            .lines()
            .filter(|line| glob_matches(pattern, line))
            .map(|s| s.to_string())
            .collect();
        for file in &matching {
            // ignore errors (file may no longer be staged)
            let _ = run_git(&["reset", "--", file.as_str()], cwd).await;
        }
    }

    let status = run_git(&["diff", "--cached", "--quiet"], cwd).await?;
    let has_staged = status.exit_code != 0;
    Ok(json!({ "has_staged": has_staged }))
}

/// Minimal glob: supports `*` as wildcard.
/// Handles patterns like "test_results.*" or "*.log".
fn glob_matches(pattern: &str, s: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix(".*") {
        let basename = s.rsplit('/').next().unwrap_or(s);
        return basename.starts_with(&format!("{prefix}."));
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return s.ends_with(&format!(".{suffix}"));
    }
    s == pattern
}

async fn execute_commit(message: &str, allow_empty: bool, cwd: &Path) -> Result<Value, AppError> {
    let staged = run_git(&["diff", "--cached", "--quiet"], cwd).await?;
    let nothing_staged = staged.exit_code == 0;

    if nothing_staged && !allow_empty {
        return Ok(json!({ "committed": false, "skipped": true, "precommit_failed": false }));
    }

    let mut args = vec!["commit", "-m", message];
    if allow_empty {
        args.push("--allow-empty");
    }
    let result = run_git(&args, cwd).await?;

    if result.exit_code == 0 {
        return Ok(json!({ "committed": true, "skipped": false, "precommit_failed": false }));
    }

    // Detect pre-commit hook failure: exit code 1 with hook-related output,
    // or any non-zero exit from git commit.
    let combined = format!("{}\n{}", result.stdout, result.stderr);
    if combined.contains("hook") || combined.contains("pre-commit") || result.exit_code == 1 {
        return Ok(json!({ "committed": false, "skipped": false, "precommit_failed": true }));
    }

    Err(AppError::new(
        ErrorCategory::ToolExecutionError,
        format!(
            "git commit failed (exit {}): {}",
            result.exit_code,
            result.stderr.trim()
        ),
    )
    .with_code("WFG-GIT-003"))
}

async fn execute_push(
    remote: &str,
    set_upstream: bool,
    retry_count: u32,
    retry_delay_ms: u64,
    cwd: &Path,
) -> Result<Value, AppError> {
    let mut args = vec!["push"];
    if set_upstream {
        args.push("-u");
    }
    args.push(remote);
    args.push("HEAD");

    let attempts = retry_count.max(1);
    let mut last_err: Option<AppError> = None;

    for attempt in 0..attempts {
        if attempt > 0 && retry_delay_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(retry_delay_ms)).await;
        }
        match run_git(&args, cwd).await {
            Ok(out) if out.exit_code == 0 => {
                return Ok(json!({ "ok": true, "message": out.stderr.trim() }));
            }
            Ok(out) => {
                last_err = Some(
                    AppError::new(
                        ErrorCategory::ToolExecutionError,
                        format!(
                            "git push failed (exit {}): {}",
                            out.exit_code,
                            out.stderr.trim()
                        ),
                    )
                    .with_code("WFG-GIT-004"),
                );
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        AppError::new(
            ErrorCategory::ToolExecutionError,
            "git push failed after retries",
        )
        .with_code("WFG-GIT-004")
    }))
}

async fn execute_diff(base: &str, max_bytes: u64, cwd: &Path) -> Result<Value, AppError> {
    let stat_ref = format!("{base}...HEAD");
    let stat_out = run_git_ok(&["diff", "--stat", &stat_ref], cwd).await?;

    let patch_ref = format!("{base}...HEAD");
    let patch_out = run_git_ok(&["-c", "core.pager=", "diff", "-U3", &patch_ref], cwd).await?;

    let patch_bytes = patch_out.stdout.len() as u64;
    let patch = if patch_bytes > max_bytes {
        patch_out.stdout[..max_bytes as usize].to_string()
    } else {
        patch_out.stdout.clone()
    };

    Ok(json!({
        "stat": stat_out.stdout,
        "patch": patch,
        "bytes": patch_bytes,
    }))
}

async fn execute_cleanup_merge(cwd: &Path) -> Result<Value, AppError> {
    // Detect in-progress merge/rebase/cherry-pick
    let git_dir_out = run_git_ok(&["rev-parse", "--git-dir"], cwd).await?;
    let git_dir = git_dir_out.stdout.trim().to_string();
    let git_dir_path = if git_dir.starts_with('/') {
        std::path::PathBuf::from(&git_dir)
    } else {
        cwd.join(&git_dir)
    };

    let in_progress = git_dir_path.join("MERGE_HEAD").exists()
        || git_dir_path.join("rebase-merge").exists()
        || git_dir_path.join("rebase-apply").exists()
        || git_dir_path.join("CHERRY_PICK_HEAD").exists();

    if in_progress {
        return Ok(json!({
            "ok": false,
            "message": "unfinished merge/rebase/cherry-pick — resolve conflicts or abort, then rerun"
        }));
    }

    // Get current branch
    let branch_out = run_git_ok(&["branch", "--show-current"], cwd).await?;
    let branch = branch_out.stdout.trim().to_string();

    run_git_ok(&["checkout", "main"], cwd).await?;
    run_git_ok(&["pull", "--rebase", "origin", "main"], cwd).await?;

    if !branch.is_empty() && branch != "main" {
        // Delete local branch (ignore error if already gone)
        let _ = run_git(&["branch", "-d", &branch], cwd).await;
        // Delete remote branch (ignore error)
        let _ = run_git(&["push", "origin", "--delete", &branch], cwd).await;
    }

    Ok(json!({ "ok": true, "message": format!("cleaned up branch: {branch}") }))
}

// ─── Operator impl ────────────────────────────────────────────────────────────

#[async_trait]
impl Operator for GitOperator {
    fn name(&self) -> &'static str {
        "GitOperator"
    }

    fn validate_params(&self, params: &Value) -> Result<(), AppError> {
        let parsed: GitParams = serde_json::from_value(params.clone()).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("GitOperator params invalid: {e}"),
            )
        })?;

        match &parsed {
            GitParams::CreateBranch { name } => {
                if name.trim().is_empty() {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        "GitOperator create_branch: name must not be empty",
                    )
                    .with_code("WFG-GIT-010"));
                }
            }
            GitParams::Commit { message, .. } => {
                if message.trim().is_empty() {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        "GitOperator commit: message must not be empty",
                    )
                    .with_code("WFG-GIT-011"));
                }
            }
            GitParams::Diff { base, .. } => {
                if base.trim().is_empty() {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        "GitOperator diff: base must not be empty",
                    )
                    .with_code("WFG-GIT-012"));
                }
            }
            GitParams::Push { remote, .. } => {
                if remote.trim().is_empty() || remote.contains(' ') || remote.starts_with('-') {
                    return Err(AppError::new(
                        ErrorCategory::ValidationError,
                        "GitOperator push: remote must be a valid identifier",
                    )
                    .with_code("WFG-GIT-013"));
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn params_schema(&self) -> schemars::Schema {
        schemars::schema_for!(GitParams)
    }

    fn output_schema(&self) -> schemars::Schema {
        // Output is operation-discriminated — permissive schema
        serde_json::from_value::<schemars::Schema>(serde_json::json!({"type": "object"}))
            .unwrap_or_default()
    }

    async fn execute(&self, params: Value, ctx: ExecutionContext) -> Result<Value, AppError> {
        let parsed: GitParams = serde_json::from_value(params).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("GitOperator params invalid: {e}"),
            )
        })?;

        let cwd = &ctx.workspace_path;

        match parsed {
            GitParams::CleanCheck {} => execute_clean_check(cwd).await,
            GitParams::SyncMain {} => execute_sync_main(cwd).await,
            GitParams::CreateBranch { name } => execute_create_branch(&name, cwd).await,
            GitParams::Stage { exclude } => execute_stage(&exclude, cwd).await,
            GitParams::Commit {
                message,
                allow_empty,
            } => execute_commit(&message, allow_empty, cwd).await,
            GitParams::Push {
                remote,
                set_upstream,
                retry_count,
                retry_delay_ms,
            } => execute_push(&remote, set_upstream, retry_count, retry_delay_ms, cwd).await,
            GitParams::Diff { base, max_bytes } => execute_diff(&base, max_bytes, cwd).await,
            GitParams::CleanupMerge {} => execute_cleanup_merge(cwd).await,
        }
    }
}
