use newton_core::workflow::executor::{ExecutionOverrides, GraphHandle};
use newton_core::workflow::operator::{ExecutionContext, Operator, OperatorRegistry, StateView};
use newton_core::workflow::operators::{self, BuiltinOperatorDeps};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use tempfile::{tempdir, TempDir};

fn build_registry() -> OperatorRegistry {
    let workspace = tempdir().expect("workspace");
    let mut builder = OperatorRegistry::builder();
    operators::register_builtins_with_deps(
        &mut builder,
        workspace.path().to_path_buf(),
        Default::default(),
        BuiltinOperatorDeps::default(),
    );
    builder.build()
}

/// GitOperator is registered and can be found by name.
#[test]
fn git_operator_is_registered() {
    let registry = build_registry();
    let op = registry.get("GitOperator");
    assert!(op.is_some(), "GitOperator must be registered");
    assert_eq!(op.unwrap().name(), "GitOperator");
}

/// params_schema() returns a valid JSON object (not null / empty).
#[test]
fn git_operator_params_schema_is_object() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();
    let schema = op.params_schema();
    let json: Value = serde_json::to_value(&schema).expect("schema must serialize");
    assert!(
        json.is_object(),
        "params_schema must serialize to a JSON object, got: {json}"
    );
    // The schema must have some content — at minimum a type or oneOf/anyOf key.
    let obj = json.as_object().unwrap();
    assert!(!obj.is_empty(), "params_schema must not be an empty object");
}

/// output_schema() returns a valid JSON object.
#[test]
fn git_operator_output_schema_is_object() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();
    let schema = op.output_schema();
    let json: Value = serde_json::to_value(&schema).expect("output schema must serialize");
    assert!(
        json.is_object(),
        "output_schema must serialize to a JSON object, got: {json}"
    );
}

/// validate_params rejects an unknown operation.
#[test]
fn git_operator_validate_rejects_unknown_operation() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();
    let params = serde_json::json!({ "operation": "does_not_exist" });
    assert!(
        op.validate_params(&params).is_err(),
        "unknown operation must fail validation"
    );
}

/// validate_params accepts each known operation with minimal/default params.
#[test]
fn git_operator_validate_accepts_known_operations() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();

    let cases = vec![
        serde_json::json!({ "operation": "clean_check" }),
        serde_json::json!({ "operation": "sync_main" }),
        serde_json::json!({ "operation": "create_branch", "name": "feature/x" }),
        serde_json::json!({ "operation": "stage" }),
        serde_json::json!({ "operation": "commit", "message": "test commit" }),
        serde_json::json!({ "operation": "push" }),
        serde_json::json!({ "operation": "diff" }),
        serde_json::json!({ "operation": "cleanup_merge" }),
    ];

    for params in &cases {
        assert!(
            op.validate_params(params).is_ok(),
            "valid params must pass validation: {params}"
        );
    }
}

/// validate_params rejects create_branch with empty name.
#[test]
fn git_operator_validate_rejects_empty_branch_name() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();
    let params = serde_json::json!({ "operation": "create_branch", "name": "" });
    let err = op
        .validate_params(&params)
        .expect_err("empty name must fail");
    assert_eq!(err.code, "WFG-GIT-010");
}

/// validate_params rejects commit with empty message.
#[test]
fn git_operator_validate_rejects_empty_commit_message() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();
    let params = serde_json::json!({ "operation": "commit", "message": "" });
    let err = op
        .validate_params(&params)
        .expect_err("empty message must fail");
    assert_eq!(err.code, "WFG-GIT-011");
}

/// validate_params rejects push with invalid remote.
#[test]
fn git_operator_validate_rejects_invalid_remote() {
    use newton_core::workflow::operators::git::GitOperator;

    let op = GitOperator::new();

    let bad_remotes = vec!["", "-origin", "bad remote"];
    for remote in bad_remotes {
        let params = serde_json::json!({ "operation": "push", "remote": remote });
        assert!(
            op.validate_params(&params).is_err(),
            "invalid remote {remote:?} must fail validation"
        );
    }
}

// ── B8: honest git-commit classification ─────────────────────────────────
//
// `execute_commit` must classify a nonzero `git commit` exit as
// `precommit_failed` ONLY on a positive hook signature (exit code 1 AND an
// actual executable hook installed in the repo) — any other nonzero exit
// (bad config, index lock, unresolved conflicts, ...) must propagate as a
// hard `Err` instead of being silently reported as a swallowed pre-commit
// rejection. See spec 074 / audit finding B8.

/// Run a `git` subcommand synchronously against `repo`, panicking with
/// stdout+stderr on failure. Used only for fixture setup, not for the
/// behavior under test.
fn run_git_sync(repo: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn git {args:?}: {e}"));
    assert!(
        output.status.success(),
        "git {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Initialize a throwaway git repo with a local identity configured (so
/// commits succeed without depending on any global/user gitconfig) and one
/// committed file, so `main`/`master`-independent commit tests have a
/// non-empty history to build on.
fn init_repo() -> TempDir {
    let dir = tempdir().expect("tempdir");
    run_git_sync(dir.path(), &["init", "-q"]);
    run_git_sync(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git_sync(dir.path(), &["config", "user.name", "Test User"]);
    std::fs::write(dir.path().join("README.md"), "init\n").unwrap();
    run_git_sync(dir.path(), &["add", "README.md"]);
    run_git_sync(dir.path(), &["commit", "-q", "-m", "init"]);
    dir
}

fn make_git_ctx(repo: &TempDir) -> ExecutionContext {
    ExecutionContext {
        workspace_path: repo.path().to_path_buf(),
        execution_id: "test-exec-git-b8".to_string(),
        task_id: "git_commit".to_string(),
        iteration: 1,
        state_view: StateView::new(
            serde_json::json!({}),
            serde_json::json!({}),
            serde_json::json!({}),
        ),
        graph: GraphHandle::new(HashMap::new()),
        workflow_file: repo.path().join("workflow.yaml"),
        nesting_depth: 0,
        execution_overrides: ExecutionOverrides::default(),
        operator_registry: OperatorRegistry::new(),
    }
}

/// A clean commit (no hooks, nothing blocking it) reports `committed: true`.
#[tokio::test]
async fn git_commit_clean_commit_succeeds() {
    use newton_core::workflow::operators::git::GitOperator;

    let repo = init_repo();
    std::fs::write(repo.path().join("a.txt"), "hello\n").unwrap();
    run_git_sync(repo.path(), &["add", "a.txt"]);

    let op = GitOperator::new();
    let ctx = make_git_ctx(&repo);
    let params = serde_json::json!({ "operation": "commit", "message": "clean commit" });
    let result = op.execute(params, ctx).await.expect("commit must succeed");

    assert_eq!(result["committed"], Value::Bool(true));
    assert_eq!(result["skipped"], Value::Bool(false));
    assert_eq!(result["precommit_failed"], Value::Bool(false));
}

/// A commit rejected by an actual pre-commit hook is reported as
/// `Ok({committed: false, precommit_failed: true})`, matching the
/// `develop.yaml` contract (`fix_snapshot_precommit` /
/// `fix_final_precommit` transitions key off `precommit_failed == true`).
#[tokio::test]
async fn git_commit_rejected_by_precommit_hook_reports_precommit_failed() {
    use newton_core::workflow::operators::git::GitOperator;

    let repo = init_repo();

    let hooks_dir = repo.path().join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let hook_path = hooks_dir.join("pre-commit");
    std::fs::write(
        &hook_path,
        "#!/bin/sh\necho 'rejected by pre-commit' >&2\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    std::fs::write(repo.path().join("b.txt"), "hello\n").unwrap();
    run_git_sync(repo.path(), &["add", "b.txt"]);

    let op = GitOperator::new();
    let ctx = make_git_ctx(&repo);
    let params = serde_json::json!({ "operation": "commit", "message": "should be rejected" });
    let result = op
        .execute(params, ctx)
        .await
        .expect("hook rejection must be Ok, not Err");

    assert_eq!(result["committed"], Value::Bool(false));
    assert_eq!(result["skipped"], Value::Bool(false));
    assert_eq!(result["precommit_failed"], Value::Bool(true));

    // Confirm nothing was actually committed.
    let log = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let log_text = String::from_utf8_lossy(&log.stdout);
    assert_eq!(
        log_text.lines().count(),
        1,
        "only the fixture's initial commit should exist, got: {log_text}"
    );
}

/// A `git commit` failure for a non-hook reason (here: `index.lock` held by
/// another process, forcing `exit 128`) must propagate as a hard `Err` —
/// not be misreported as `precommit_failed`. This is the exact bug in B8:
/// the old classifier's `exit_code == 1` fallback would have swallowed any
/// nonzero exit as a fabricated pre-commit rejection; `index.lock` and
/// config errors exit 128, and this repo has no hooks installed at all, so
/// the positive-hook-signature classifier must not fire.
#[tokio::test]
async fn git_commit_non_hook_failure_returns_err() {
    use newton_core::workflow::operators::git::GitOperator;

    let repo = init_repo();
    std::fs::write(repo.path().join("c.txt"), "hello\n").unwrap();
    run_git_sync(repo.path(), &["add", "c.txt"]);

    // Simulate a concurrent git process holding the index lock.
    let lock_path = repo.path().join(".git").join("index.lock");
    std::fs::write(&lock_path, "").unwrap();

    let op = GitOperator::new();
    let ctx = make_git_ctx(&repo);
    let params = serde_json::json!({ "operation": "commit", "message": "should hard-fail" });
    let err = op
        .execute(params, ctx)
        .await
        .expect_err("index-lock failure must be a hard Err, not a swallowed precommit_failed");

    assert_eq!(err.code, "WFG-GIT-003");

    std::fs::remove_file(&lock_path).unwrap();
}

/// Fix 4 (B8 exclusions): a "nothing to commit" `git commit` exit — the
/// narrow TOCTOU where nothing is staged at commit time — must never be
/// misclassified as `precommit_failed`, even with a hook installed and
/// executable in the repo. Here the up-front `diff --cached --quiet` check
/// already catches the (non-racy) "nothing staged" case and short-circuits
/// before `git commit` (and therefore the hook) ever runs — proven by the
/// hook writing a marker file that must NOT exist afterward — but the
/// invariant under test (hook presence must never turn a legitimate
/// "nothing to commit" outcome into `precommit_failed`) is the same one the
/// new git-output-based classification in `execute_commit` protects for the
/// genuine TOCTOU race (something unstages the change between the check and
/// the actual `git commit` call), which cannot be constructed
/// deterministically in a single-process test.
#[tokio::test]
async fn git_commit_nothing_staged_with_hook_installed_is_skipped_not_precommit_failed() {
    use newton_core::workflow::operators::git::GitOperator;

    let repo = init_repo();

    let hooks_dir = repo.path().join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let hook_path = hooks_dir.join("pre-commit");
    let marker_path = repo.path().join("hook_ran_marker");
    std::fs::write(
        &hook_path,
        format!(
            "#!/bin/sh\ntouch '{}'\nexit 0\n",
            marker_path.to_string_lossy()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // Nothing staged — `init_repo` already committed everything.
    let op = GitOperator::new();
    let ctx = make_git_ctx(&repo);
    let params = serde_json::json!({ "operation": "commit", "message": "nothing to commit here" });
    let result = op
        .execute(params, ctx)
        .await
        .expect("nothing-to-commit must be Ok, not Err");

    assert_eq!(result["committed"], Value::Bool(false));
    assert_eq!(result["skipped"], Value::Bool(true));
    assert_eq!(
        result["precommit_failed"],
        Value::Bool(false),
        "an installed hook must never turn a legitimate nothing-to-commit outcome into \
         precommit_failed; result={result}"
    );
    assert!(
        !marker_path.exists(),
        "hook must not have run at all — the operator should short-circuit before spawning \
         `git commit` when nothing is staged"
    );
}

/// Fix 4 (B8 exclusions): "Aborting commit due to empty commit message" is
/// git's other well-known non-hook exit-1 cause. It's reachable through this
/// operator's exact `git commit -m <message>` invocation (no `--cleanup`
/// flag passed) when the repo has `commit.cleanup = strip` configured and
/// the message is comment-only lines — a message that passes this
/// operator's own `WFG-GIT-011` non-empty-after-trim validation but still
/// strips to nothing under git's own cleanup. Must propagate as a hard
/// `WFG-GIT-003` `Err`, not be silently swallowed or misclassified as
/// `precommit_failed` — this repo has no hooks installed, so the
/// hook-presence classifier must not fire regardless.
#[tokio::test]
async fn git_commit_empty_message_after_cleanup_strip_is_hard_err() {
    use newton_core::workflow::operators::git::GitOperator;

    let repo = init_repo();
    run_git_sync(repo.path(), &["config", "commit.cleanup", "strip"]);

    std::fs::write(repo.path().join("d.txt"), "hello\n").unwrap();
    run_git_sync(repo.path(), &["add", "d.txt"]);

    let op = GitOperator::new();
    let ctx = make_git_ctx(&repo);
    // Non-empty after `.trim()` (passes WFG-GIT-011 validation) but strips
    // to nothing under `commit.cleanup = strip`, since it's comment-only.
    let params = serde_json::json!({ "operation": "commit", "message": "# just a comment" });
    let err = op
        .execute(params, ctx)
        .await
        .expect_err("empty-after-cleanup message must be a hard Err, not Ok");

    assert_eq!(err.code, "WFG-GIT-003");

    // Confirm nothing was actually committed.
    let log = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let log_text = String::from_utf8_lossy(&log.stdout);
    assert_eq!(
        log_text.lines().count(),
        1,
        "only the fixture's initial commit should exist, got: {log_text}"
    );
}
