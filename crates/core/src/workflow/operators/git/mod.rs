#![allow(clippy::result_large_err)]

use crate::core::error::AppError;
use crate::core::types::ErrorCategory;
use crate::workflow::operator::{ExecutionContext, Operator};
use crate::workflow::subprocess::run_guarded;
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

    // `run_guarded` spawns `cmd` as the leader of its own process group with
    // `kill_on_drop(true)` and guards it with `ProcessGroupKillGuard`, so an
    // outer task timeout dropping this future can't orphan a grandchild git
    // spawns (e.g. a hook backgrounding work). See `workflow::subprocess`.
    let output = run_guarded(cmd).await.map_err(|e| {
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

    if !exclude.is_empty() {
        // Build the GlobSet once, up front — not per-line — so real glob
        // syntax (`build/*`, `**/*.log`, `dir/*.tmp`) is matched correctly
        // and efficiently, replacing the old hand-rolled prefix/suffix-only
        // matcher (spec 074 P11).
        let exclude_set = build_exclude_glob_set(exclude)?;
        let listed = run_git(&["diff", "--cached", "--name-only"], cwd).await?;
        let matching: Vec<String> = listed
            .stdout
            .lines()
            .filter(|line| exclude_set.is_match(line))
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

/// Builds a `globset::GlobSet` from the `exclude` pattern list, so
/// `execute_stage` can test "does this line match any exclude pattern" in
/// one call per line instead of re-parsing patterns per line. Uses real glob
/// syntax (`build/*`, `**/*.log`, `dir/*.tmp`) via the `globset` crate
/// (spec 074 P11), replacing the previous hand-rolled matcher that only
/// understood `*.ext` / `name.*`.
fn build_exclude_glob_set(exclude: &[String]) -> Result<globset::GlobSet, AppError> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in exclude {
        // A pattern with no path separator (e.g. `test_results.*`) is a
        // basename pattern: it must match that name at any depth, matching
        // the pre-globset matcher's basename-extraction behavior. `globset`
        // anchors a bare pattern to the full relative path, so without this
        // `**/` prefix `test_results.*` would only match a root-level file
        // and silently stop excluding the same name inside a subdirectory —
        // exactly the regression this repo's own `git_stage` exclude lists
        // (e.g. `test_results.*` in develop.yaml) would hit.
        let effective_pattern = if pattern.contains('/') {
            pattern.clone()
        } else {
            format!("**/{pattern}")
        };
        let glob = globset::Glob::new(&effective_pattern).map_err(|e| {
            AppError::new(
                ErrorCategory::ValidationError,
                format!("GIT-STAGE-001: invalid exclude glob pattern {pattern:?}: {e}"),
            )
            .with_code("GIT-STAGE-001")
        })?;
        builder.add(glob);
    }
    builder.build().map_err(|e| {
        AppError::new(
            ErrorCategory::ValidationError,
            format!("GIT-STAGE-002: failed to build exclude glob set: {e}"),
        )
        .with_code("GIT-STAGE-002")
    })
}

/// Positive structural signature for `execute_commit`'s classification: is
/// there an executable `pre-commit` or `commit-msg` hook actually installed
/// in this repository? Resolved via `git rev-parse --git-path hooks/<name>`
/// so a `core.hooksPath` override (relative or absolute) is honored the
/// same way git itself resolves it.
///
/// Spec 074 / B8: `git commit` normalizes ANY pre-commit/commit-msg hook
/// rejection to exit code 1 — regardless of the hook script's own exit
/// code — while genuine git-level failures (bad `user.email`/`user.name`,
/// `index.lock` held, unresolved merge conflicts, bad flags) exit 128
/// (fatal) or 129 (usage). But exit code 1 alone is not a reliable hook
/// signal: git also returns 1 for "nothing to commit" (a narrow TOCTOU race
/// against the earlier `diff --cached --quiet` check) and for "Aborting
/// commit due to empty commit message" (if `-m`'s message strips to empty
/// under git's default `--cleanup=strip`). Requiring an actual installed
/// hook as well closes that gap without resorting to fragile substring
/// matching on hook output (which can be empty — a hook is free to fail
/// silently).
async fn repo_has_commit_hook(cwd: &Path) -> bool {
    for hook_name in ["pre-commit", "commit-msg"] {
        let git_path_arg = format!("hooks/{hook_name}");
        let Ok(path_out) = run_git(&["rev-parse", "--git-path", &git_path_arg], cwd).await else {
            continue;
        };
        if path_out.exit_code != 0 {
            continue;
        }
        let hook_rel = path_out.stdout.trim();
        if hook_rel.is_empty() {
            continue;
        }
        if is_executable_file(&cwd.join(hook_rel)) {
            return true;
        }
    }
    false
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    // No portable executable-bit check; treat any regular file at the
    // resolved hook path as a hook, matching git's own non-unix behavior
    // closely enough for this classifier's purposes.
    path.is_file()
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

    // git's own two well-known non-hook exit-1 causes (see the doc comment
    // above) — checked BEFORE the hook-presence classification below so an
    // installed hook doesn't cause either of these to be misclassified as
    // `precommit_failed`.
    let combined_output = format!("{}\n{}", result.stdout, result.stderr).to_lowercase();

    if combined_output.contains("nothing to commit") {
        // TOCTOU: the up-front `diff --cached --quiet` check raced with
        // something unstaging the change before `git commit` actually ran.
        // Same shape as the up-front clean-tree skip path above.
        return Ok(json!({ "committed": false, "skipped": true, "precommit_failed": false }));
    }

    if combined_output.contains("aborting commit due to empty commit message") {
        return Err(AppError::new(
            ErrorCategory::ToolExecutionError,
            format!(
                "git commit aborted: empty commit message after cleanup (exit {}): {}",
                result.exit_code,
                result.stderr.trim()
            ),
        )
        .with_code("WFG-GIT-003"));
    }

    // Positive-signature classification only (spec 074 / B8): exit code 1
    // AND an actual hook installed. Any other nonzero exit — including
    // exit code 1 with no hook present — is a hard `Err`, never silently
    // swallowed as a fabricated pre-commit rejection.
    if result.exit_code == 1 && repo_has_commit_hook(cwd).await {
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
        let end = patch_out.stdout.floor_char_boundary(max_bytes as usize);
        patch_out.stdout[..end].to_string()
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

#[cfg(test)]
mod exclude_glob_tests {
    use super::build_exclude_glob_set;

    fn matches(patterns: &[&str], line: &str) -> bool {
        let patterns: Vec<String> = patterns.iter().map(|s| s.to_string()).collect();
        build_exclude_glob_set(&patterns)
            .expect("valid glob set")
            .is_match(line)
    }

    // ── Real glob forms the old hand-rolled matcher silently failed on ──────

    #[test]
    fn matches_directory_star() {
        assert!(matches(&["build/*"], "build/output.bin"));
        assert!(matches(&["build/*"], "build/nested/output.bin"));
        assert!(!matches(&["build/*"], "src/output.bin"));
    }

    #[test]
    fn matches_double_star_extension() {
        assert!(matches(&["**/*.log"], "app.log"));
        assert!(matches(&["**/*.log"], "logs/app.log"));
        assert!(matches(&["**/*.log"], "a/b/c/app.log"));
        assert!(!matches(&["**/*.log"], "app.txt"));
    }

    #[test]
    fn matches_dir_prefixed_extension() {
        assert!(matches(&["dir/*.tmp"], "dir/scratch.tmp"));
        assert!(!matches(&["dir/*.tmp"], "other/scratch.tmp"));
        assert!(!matches(&["dir/*.tmp"], "dir/scratch.log"));
    }

    // ── Regression: the two forms the old matcher already supported ────────

    #[test]
    fn matches_suffix_extension_form() {
        assert!(matches(&["*.ext"], "file.ext"));
        assert!(matches(&["*.ext"], "nested/file.ext"));
        assert!(!matches(&["*.ext"], "file.txt"));
    }

    #[test]
    fn matches_prefix_dot_star_form() {
        assert!(matches(&["name.*"], "name.json"));
        assert!(!matches(&["name.*"], "other.json"));
    }

    /// Regression: a bare `name.*` (or any pattern with no `/`) must still
    /// match at any depth, not just at the repo root — `globset::Glob`
    /// anchors an unmodified pattern to the full relative path, which would
    /// otherwise silently stop `test_results.*`-style excludes (used by this
    /// repo's own `develop.yaml` workflows) from matching nested files.
    #[test]
    fn bare_pattern_matches_at_any_depth() {
        assert!(matches(&["test_results.*"], "test_results.json"));
        assert!(matches(&["test_results.*"], "artifacts/test_results.json"));
        assert!(matches(&["test_results.*"], "a/b/c/test_results.xml"));
        assert!(!matches(&["test_results.*"], "other.json"));

        assert!(matches(&["*.log"], "app.log"));
        assert!(matches(&["*.log"], "logs/app.log"));
    }

    // ── Multi-pattern exclude lists (the real call shape from execute_stage) ─

    #[test]
    fn multiple_patterns_match_any() {
        let patterns = vec!["build/*", "**/*.log", "dir/*.tmp"];
        assert!(matches(&patterns, "build/x"));
        assert!(matches(&patterns, "a/b.log"));
        assert!(matches(&patterns, "dir/c.tmp"));
        assert!(!matches(&patterns, "src/main.rs"));
    }

    #[test]
    fn empty_pattern_list_matches_nothing() {
        assert!(!matches(&[], "anything.txt"));
    }
}
