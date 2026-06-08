"""
Round-trip test for develop.yaml.

Authors the workflow in Python using the newton-dsl, compiles to YAML,
validates with newton, and asserts semantic equality with the conformance fixture.

develop.yaml is the most complex workflow:
- Multiple overlapping cycles (non-nested)
- GhOperator, AgentOperator with signals, CommandOperator
- Typed refs: t.out.stdout, t.out.signal, t.out.pr_number, t.out.state, t.out.stdout_artifact
- Injected vars: develop_primary_engine, develop_primary_model, develop_secondary_engine, develop_secondary_model
- artifact_storage configured
- Multiple terminal tasks
"""
from __future__ import annotations

import warnings

import pytest
import yaml

from newton import Workflow, agent, command, gh, when, expr
from newton.refs import AmbientRef

from .conftest import (
    validate_with_newton,
    load_fixture,
    tasks_semantic_equal,
    normalize_yaml,
)


def build_develop() -> Workflow:
    """Author develop.yaml using the newton-dsl."""
    artifact_storage = {
        "base_path": ".newton/artifacts",
        "max_inline_bytes": 4194304,
        "max_artifact_bytes": 104857600,
        "max_total_bytes": 1073741824,
        "retention_hours": 168,
        "cleanup_policy": "lru",
    }

    wf = Workflow(
        "rust-workflow",
        description=(
            "Implement from a repo-local spec path. Prefer raw_spec_path + board_issue_number + "
            "board_item_title (develop.sh); workflow copies into tmp/<issue>-<slug>.md before git work."
        ),
        default_engine="opencode",
        parallel_limit=1,
        max_time_seconds=604800,
        continue_on_error=False,
        max_task_iterations=15000,
        max_workflow_iterations=500,
        allow_shell=True,
        artifact_storage=artifact_storage,
    )

    wf.inputs(
        prompt="",
        raw_spec_path="",
        board_item_id="",
        board_issue_number="",
        board_item_title="",
        skip_gh_pr_approve="true",
    )

    wf.expects(
        "develop_primary_engine",
        "develop_primary_model",
        "develop_secondary_engine",
        "develop_secondary_model",
    )

    # Context
    wf.set_context(
        preamble=(
            "Implement the following spec in the target source code target.\n"
            "After you have applied the changes and ./scripts/run-tests.sh completes with no errors, "
            "print exactly <status>COMPLETED</status> in your final output."
        ),
        codescene_preamble=(
            "Fix code health issues reported by CodeScene. Work in the target source code base; "
            "run ./scripts/run-tests.sh to verify. Do not require a COMPLETED marker."
        ),
        precommit_fix_preamble=(
            "The pre-commit hook rejected a commit. The full hook output is below.\n"
            "Fix all reported issues in the codebase. Run ./scripts/run-tests.sh to verify.\n"
            "Do not attempt to commit; only fix the code."
        ),
        validation_preamble=(
            "You are a reviewer. You will be given the spec path (read the spec file in the workspace) "
            "and a bounded diff vs main (stat plus truncated patch).\n"
            "Determine whether the provided diff fully implements the given spec.\n"
            "Reply with exactly one of <status>VALID</status> or <status>INVALID</status>.\n"
            "If INVALID, include a single block <feedback>...</feedback> with clear, actionable feedback."
        ),
    )

    # Ambient refs for injected vars
    primary_engine = AmbientRef("develop_primary_engine")
    primary_model = AmbientRef("develop_primary_model")
    secondary_engine = AmbientRef("develop_secondary_engine")
    secondary_model = AmbientRef("develop_secondary_model")

    # ----------------------------------------------------------------
    # ensure_clean_main
    # ----------------------------------------------------------------
    ensure_clean_main = wf.task(
        "ensure_clean_main",
        command(
            (
                "set -e\n"
                "git diff --quiet || { echo \"Precondition failed: unstaged changes. "
                "Commit or stash before develop.\"; exit 1; }\n"
                "git diff --cached --quiet || { echo \"Precondition failed: staged changes. "
                "Commit or stash before develop.\"; exit 1; }\n"
                "git fetch origin\n"
                "git checkout main\n"
                "git pull --rebase origin main\n"
            ),
            shell=True,
            capture_stdout=False,
        ),
        name="Ensure clean worktree",
    )

    # ----------------------------------------------------------------
    # resolve_board_ids
    # ----------------------------------------------------------------
    resolve_board_ids = wf.task(
        "resolve_board_ids",
        gh.project_resolve_board(
            owner=wf.env("GH_PROJECT_OWNER"),
            project_number=wf.env("GH_PROJECT_NUMBER"),
        ),
        name="Pick next Issue",
    )

    # ----------------------------------------------------------------
    # move_to_in_progress
    # ----------------------------------------------------------------
    move_to_in_progress = wf.task(
        "move_to_in_progress",
        gh.project_item_set_status(
            item_id=wf.input.board_item_id,
            board=resolve_board_ids.output,
            status="In progress",
            on_error="warn",
        ),
        name="Move Issue to In Progress",
    )

    # ----------------------------------------------------------------
    # prepare_spec_paths
    # ----------------------------------------------------------------
    prepare_spec_paths = wf.task(
        "prepare_spec_paths",
        command(
            (
                "set -eo pipefail\n"
                "mkdir -p .newton/plan tmp\n"
                "slugify() {\n"
                "  s=$(printf '%s' \"${TITLE:-}\" | iconv -f utf-8 -t ascii//TRANSLIT 2>/dev/null || "
                "printf '%s' \"${TITLE:-}\")\n"
                "  s=$(printf '%s' \"$s\" | tr '[:upper:]' '[:lower:]' | "
                "sed -E 's/[^a-z0-9]+/-/g; s/^-+|-+$//g')\n"
                "  [ -z \"$s\" ] && s=\"spec\"\n"
                "  printf '%s' \"$s\" | cut -c1-60 | sed -E 's/-+$//'\n"
                "}\n"
                "if [ -n \"$RAW_SPEC\" ] && [ -f \"$RAW_SPEC\" ]; then\n"
                "  slug=$(slugify)\n"
                "  OUT=\"tmp/${ISSUE}-${slug}.md\"\n"
                "  if [ \"$RAW_SPEC\" != \"$OUT\" ]; then\n"
                "    cp \"$RAW_SPEC\" \"$OUT\"\n"
                "  fi\n"
                "else\n"
                "  OUT=\"$LEGACY_PROMPT\"\n"
                "  if [ -z \"$OUT\" ] || [ ! -f \"$OUT\" ]; then\n"
                "    echo \"prepare_spec_paths: set raw_spec_path to an existing file or pass prompt "
                "with a readable path\"\n"
                "    exit 1\n"
                "  fi\n"
                "fi\n"
                "printf '%s\\n' \"$OUT\" > .newton/plan/.develop-prompt-path\n"
                "echo \"spec_path=$OUT\"\n"
            ),
            shell=True,
            capture_stdout=True,
            env={
                "ISSUE": wf.input.board_issue_number,
                "TITLE": wf.input.board_item_title,
                "RAW_SPEC": wf.input.raw_spec_path,
                "LEGACY_PROMPT": wf.input.prompt,
            },
        ),
    )

    # ----------------------------------------------------------------
    # read_develop_spec_path
    # ----------------------------------------------------------------
    read_develop_spec_path = wf.task(
        "read_develop_spec_path",
        command(
            "set -eo pipefail\nprintf '%s' \"$(cat .newton/plan/.develop-prompt-path)\"\n",
            shell=True,
            capture_stdout=True,
        ),
    )

    # ----------------------------------------------------------------
    # create_branch
    # ----------------------------------------------------------------
    create_branch = wf.task(
        "create_branch",
        command(
            (
                "BRANCH_NAME=$(basename \"$PROMPT_PATH\" | sed 's/\\.[^.]*$//')\n"
                "git checkout -b \"feature/$BRANCH_NAME\"\n"
            ),
            shell=True,
            env={"PROMPT_PATH": read_develop_spec_path.out.stdout},
            capture_stdout=False,
        ),
        name="Create Branch",
    )

    # ----------------------------------------------------------------
    # load_spec
    # ----------------------------------------------------------------
    load_spec = wf.task(
        "load_spec",
        command(
            "set -e\ncat \"$SPEC_PATH\"\n",
            shell=True,
            env={"SPEC_PATH": read_develop_spec_path.out.stdout},
            capture_stdout=True,
            capture_stderr=True,
        ),
    )

    # ----------------------------------------------------------------
    # implement_spec
    # ----------------------------------------------------------------
    implement_spec = wf.task(
        "implement_spec",
        agent(
            engine=primary_engine,
            model=primary_model,
            prompt=expr(
                'context.preamble + "\\n\\nSpec path: " + tasks.read_develop_spec_path.output.stdout + '
                '"\\n\\nSpec content:\\n" + tasks.load_spec.output.stdout'
            ),
            signals={"complete": "<status>COMPLETED</status>"},
        ),
    )
    implement_spec.repeat_at_most(3)

    # ----------------------------------------------------------------
    # run_tests
    # ----------------------------------------------------------------
    run_tests = wf.task(
        "run_tests",
        command(
            (
                "OUTPUT=$(./scripts/run-tests.sh 2>&1)\n"
                "EXIT=$?\n"
                "echo \"$OUTPUT\"\n"
                "[ $EXIT -eq 0 ] && echo \"TEST_STATUS: passed\" || echo \"TEST_STATUS: failed\"\n"
            ),
            shell=True,
            capture_stdout=True,
            capture_stderr=True,
        ),
    )
    run_tests.repeat_at_most(60)

    # ----------------------------------------------------------------
    # fix_test_failures
    # ----------------------------------------------------------------
    fix_test_failures = wf.task(
        "fix_test_failures",
        agent(
            engine=secondary_engine,
            model=secondary_model,
            prompt=expr(
                'context.preamble + "\\n\\nSpec path: " + tasks.read_develop_spec_path.output.stdout + '
                '"\\n\\nSpec content:\\n" + tasks.load_spec.output.stdout + '
                '"\\n\\nTests failed. Fix the issues and re-run ./scripts/run-tests.sh:\\n" + '
                'tasks.run_tests.output.stdout'
            ),
        ),
    )
    fix_test_failures.repeat_at_most(3)

    # ----------------------------------------------------------------
    # snapshot_commit
    # ----------------------------------------------------------------
    snapshot_commit = wf.task(
        "snapshot_commit",
        command(
            (
                "SPEC_BASENAME=$(basename \"$SPEC_PATH\" | sed 's/\\.[^.]*$//')\n"
                "git add -A\n"
                "git diff --cached --name-only | { grep -E '^test_results\\.' || true; } | xargs -r git reset --\n"
                "if git diff --cached --quiet; then\n"
                "  echo \"COMMIT_STATUS: skipped\"\n"
                "  exit 0\n"
                "fi\n"
                "OUTPUT=$(git commit -m \"chore(develop): $SPEC_BASENAME\" 2>&1)\n"
                "EXIT=$?\n"
                "echo \"$OUTPUT\"\n"
                "[ $EXIT -eq 0 ] && echo \"COMMIT_STATUS: success\" || echo \"COMMIT_STATUS: failed\"\n"
            ),
            shell=True,
            env={"SPEC_PATH": read_develop_spec_path.out.stdout},
            capture_stdout=True,
            capture_stderr=True,
        ),
    )
    snapshot_commit.repeat_at_most(90)

    # ----------------------------------------------------------------
    # fix_snapshot_precommit
    # ----------------------------------------------------------------
    fix_snapshot_precommit = wf.task(
        "fix_snapshot_precommit",
        agent(
            engine=secondary_engine,
            model=secondary_model,
            prompt=expr(
                'context.precommit_fix_preamble + "\\n\\nPre-commit hook output:\\n" + '
                'tasks.snapshot_commit.output.stdout'
            ),
        ),
    )
    fix_snapshot_precommit.repeat_at_most(3)

    # ----------------------------------------------------------------
    # get_diff
    # ----------------------------------------------------------------
    get_diff = wf.task(
        "get_diff",
        command(
            (
                "set -e\n"
                "echo \"=== diff stat ===\"\n"
                "git diff --stat main...HEAD\n"
                "echo \"\"\n"
                "echo \"=== diff (bounded, max 256KiB) ===\"\n"
                "git diff -U3 main...HEAD | head -c 262144\n"
                "DIFF_BYTES=$(git diff main...HEAD | wc -c | tr -d ' ')\n"
                "echo \"\"\n"
                "echo \"=== diff_bytes=$DIFF_BYTES ===\"\n"
            ),
            shell=True,
            capture_stdout=True,
        ),
    )
    get_diff.repeat_at_most(60)

    # ----------------------------------------------------------------
    # validation_preflight
    # ----------------------------------------------------------------
    validation_preflight = wf.task(
        "validation_preflight",
        command(
            (
                "set -e\n"
                "SPEC_BYTES=$(wc -c < \"$SPEC_PATH\" | tr -d ' ')\n"
                "DIFF_BYTES=$(git diff main...HEAD | wc -c | tr -d ' ')\n"
                "PREAMBLE_BUDGET=8192\n"
                "TOTAL=$((SPEC_BYTES + DIFF_BYTES + PREAMBLE_BUDGET))\n"
                "MAX_BYTES=400000\n"
                "echo \"spec_bytes=$SPEC_BYTES diff_bytes=$DIFF_BYTES total_estimate=$TOTAL max_bytes=$MAX_BYTES\"\n"
                "if [ \"$TOTAL\" -gt \"$MAX_BYTES\" ]; then\n"
                "  echo \"VALIDATION_PREFLIGHT: oversize\"\n"
                "  exit 1\n"
                "fi\n"
                "echo \"VALIDATION_PREFLIGHT: ok\"\n"
            ),
            shell=True,
            capture_stdout=True,
            env={"SPEC_PATH": read_develop_spec_path.out.stdout},
        ),
    )

    # ----------------------------------------------------------------
    # fail_validation_oversized
    # ----------------------------------------------------------------
    fail_validation_oversized = wf.task(
        "fail_validation_oversized",
        command(
            (
                "echo \"develop: validation preflight failed — spec+diff exceeds safe prompt budget.\" >&2\n"
                "echo \"See validation_preflight output for byte counts. "
                "Split the change or spec before re-running.\" >&2\n"
                "exit 1\n"
            ),
            shell=True,
            capture_stdout=False,
        ),
    )
    fail_validation_oversized._terminal = "failure"

    # ----------------------------------------------------------------
    # validate_against_spec
    # ----------------------------------------------------------------
    validate_against_spec = wf.task(
        "validate_against_spec",
        agent(
            engine=primary_engine,
            model=primary_model,
            require_signal=True,
            prompt=expr(
                'context.validation_preamble + "\\n\\nSpec path (read in workspace): " + '
                'tasks.read_develop_spec_path.output.stdout + '
                '"\\n\\nDiff vs main (stat + bounded diff):\\n" + tasks.get_diff.output.stdout'
            ),
            signals={
                "valid": "<status>VALID</status>",
                "invalid": "<status>INVALID</status>",
            },
        ),
    )
    validate_against_spec.repeat_at_most(3)

    # ----------------------------------------------------------------
    # load_validation_feedback
    # ----------------------------------------------------------------
    load_validation_feedback = wf.task(
        "load_validation_feedback",
        command(
            'cat "$VALIDATION_STDOUT_ARTIFACT"',
            shell=True,
            capture_stdout=True,
            env={"VALIDATION_STDOUT_ARTIFACT": validate_against_spec.out.stdout_artifact},
        ),
    )

    # ----------------------------------------------------------------
    # implement_feedback
    # ----------------------------------------------------------------
    implement_feedback = wf.task(
        "implement_feedback",
        agent(
            engine=primary_engine,
            model=primary_model,
            prompt=expr(
                'context.preamble + "\\n\\nSpec path: " + tasks.read_develop_spec_path.output.stdout + '
                '"\\n\\nSpec content:\\n" + tasks.load_spec.output.stdout + '
                '"\\n\\nValidation feedback (address and re-run tests):\\n" + '
                'tasks.load_validation_feedback.output.stdout'
            ),
            signals={"complete": "<status>COMPLETED</status>"},
        ),
    )
    implement_feedback.repeat_at_most(5)

    # ----------------------------------------------------------------
    # analyze_code_health
    # ----------------------------------------------------------------
    analyze_code_health = wf.task(
        "analyze_code_health",
        command(
            "cs delta --output-format json",
            shell=True,
            capture_stdout=True,
        ),
    )

    # ----------------------------------------------------------------
    # fix_code_health
    # ----------------------------------------------------------------
    fix_code_health = wf.task(
        "fix_code_health",
        agent(
            engine=secondary_engine,
            model=secondary_model,
            prompt=expr(
                'context.codescene_preamble + "\\n\\nCodeScene delta report (JSON):\\n" + '
                'tasks.analyze_code_health.output.stdout'
            ),
        ),
    )

    # ----------------------------------------------------------------
    # git_stage
    # ----------------------------------------------------------------
    git_stage = wf.task(
        "git_stage",
        command(
            (
                "set -e\n"
                "git add -A\n"
                "git diff --cached --name-only | { grep -E '^test_results\\.' || true; } | xargs -r git reset --\n"
                "if git diff --cached --quiet; then\n"
                "  if git diff --quiet main...HEAD; then\n"
                "    echo \"NO_CHANGES\"\n"
                "  else\n"
                "    echo \"COMMITTED_ONLY\"\n"
                "  fi\n"
                "  exit 0\n"
                "fi\n"
                "echo \"HAS_CHANGES\"\n"
            ),
            shell=True,
            capture_stdout=True,
        ),
    )

    # ----------------------------------------------------------------
    # no_changes_done (terminal: success)
    # ----------------------------------------------------------------
    no_changes_done = wf.task(
        "no_changes_done",
        command(
            "echo \"No changes to commit; skipping PR.\"",
            shell=True,
            capture_stdout=False,
        ),
    )
    no_changes_done._terminal = "success"

    # ----------------------------------------------------------------
    # git_commit
    # ----------------------------------------------------------------
    git_commit = wf.task(
        "git_commit",
        command(
            (
                "MSG=\"feat: implement $(basename \"$PROMPT_PATH\" .md 2>/dev/null || echo \"spec\")\"\n"
                "if git diff --cached --quiet; then\n"
                "  echo \"No changes to commit\"\n"
                "  echo \"COMMIT_STATUS: skipped\"\n"
                "  exit 0\n"
                "fi\n"
                "OUTPUT=$(git commit -m \"$MSG\" 2>&1)\n"
                "EXIT=$?\n"
                "echo \"$OUTPUT\"\n"
                "[ $EXIT -eq 0 ] && echo \"COMMIT_STATUS: success\" || echo \"COMMIT_STATUS: failed\"\n"
            ),
            shell=True,
            env={"PROMPT_PATH": read_develop_spec_path.out.stdout},
            capture_stdout=True,
            capture_stderr=True,
        ),
    )

    # ----------------------------------------------------------------
    # fix_final_precommit
    # ----------------------------------------------------------------
    fix_final_precommit = wf.task(
        "fix_final_precommit",
        agent(
            engine=secondary_engine,
            model=secondary_model,
            prompt=expr(
                'context.precommit_fix_preamble + "\\n\\nPre-commit hook output:\\n" + '
                'tasks.git_commit.output.stdout'
            ),
        ),
    )
    fix_final_precommit.repeat_at_most(3)

    # ----------------------------------------------------------------
    # git_push
    # ----------------------------------------------------------------
    git_push = wf.task(
        "git_push",
        command(
            (
                "for i in 1 2 3; do\n"
                "  git push -u origin HEAD && exit 0\n"
                "  sleep 5\n"
                "done\n"
                "echo \"git push failed after 3 attempts\"; exit 1\n"
            ),
            shell=True,
            capture_stdout=True,
            capture_stderr=True,
        ),
    )

    # ----------------------------------------------------------------
    # gh_create_pr
    # ----------------------------------------------------------------
    gh_create_pr = wf.task(
        "gh_create_pr",
        gh.pr_create(
            base="main",
            title=expr('"feat: implement " + file_stem(tasks.read_develop_spec_path.output.stdout)'),
            body="Implements spec. Merge with squash.",
            retry_count=3,
            retry_delay_ms=5000,
        ),
    )

    # ----------------------------------------------------------------
    # move_to_in_review
    # ----------------------------------------------------------------
    move_to_in_review = wf.task(
        "move_to_in_review",
        gh.project_item_set_status(
            item_id=wf.input.board_item_id,
            board=resolve_board_ids.output,
            status="In review",
            on_error="warn",
        ),
    )

    # ----------------------------------------------------------------
    # gh_approve_pr
    # ----------------------------------------------------------------
    gh_approve_pr = wf.task(
        "gh_approve_pr",
        gh.pr_approve(pr_number=gh_create_pr.out.pr_number),
    )

    # ----------------------------------------------------------------
    # poll_pr
    # ----------------------------------------------------------------
    poll_pr = wf.task(
        "poll_pr",
        gh.pr_view(pr=gh_create_pr.out.pr_number),
        name="Wait for PR Acceptance",
    )
    poll_pr.repeat_at_most(15000)
    poll_pr.timeout(120)

    # ----------------------------------------------------------------
    # sleep_merge_wait
    # ----------------------------------------------------------------
    sleep_merge_wait = wf.task(
        "sleep_merge_wait",
        command("sleep 60", shell=True, capture_stdout=False),
    )
    sleep_merge_wait.repeat_at_most(15000)

    # ----------------------------------------------------------------
    # sleep_merge_unknown
    # ----------------------------------------------------------------
    sleep_merge_unknown = wf.task(
        "sleep_merge_unknown",
        command(
            "echo \"Unexpected PR state; waiting 30s\"; sleep 30",
            shell=True,
            capture_stdout=False,
        ),
    )
    sleep_merge_unknown.repeat_at_most(15000)

    # ----------------------------------------------------------------
    # move_to_ready_on_close
    # ----------------------------------------------------------------
    move_to_ready_on_close = wf.task(
        "move_to_ready_on_close",
        gh.project_item_set_status(
            item_id=wf.input.board_item_id,
            board=resolve_board_ids.output,
            status="Ready",
            on_error="warn",
        ),
    )

    # ----------------------------------------------------------------
    # fail_pr_closed (terminal: failure)
    # ----------------------------------------------------------------
    fail_pr_closed = wf.task(
        "fail_pr_closed",
        command(
            "echo \"PR closed without merge\" >&2; exit 1",
            shell=True,
            capture_stdout=False,
        ),
    )
    fail_pr_closed._terminal = "failure"

    # ----------------------------------------------------------------
    # merge_git_cleanup
    # ----------------------------------------------------------------
    merge_git_cleanup = wf.task(
        "merge_git_cleanup",
        command(
            (
                "set -e\n"
                "if [ -f .git/MERGE_HEAD ] || [ -d .git/rebase-merge ] || [ -d .git/rebase-apply ] || "
                "[ -f .git/CHERRY_PICK_HEAD ]; then\n"
                "  echo \"merge_git_cleanup: unfinished merge/rebase/cherry-pick (cannot checkout main). "
                "Resolve conflicts or abort, then rerun.\" >&2\n"
                "  git status >&2 || true\n"
                "  exit 1\n"
                "fi\n"
                "BRANCH=$(git branch --show-current)\n"
                "git checkout main\n"
                "git pull --rebase origin main\n"
                "git branch -d \"$BRANCH\" 2>/dev/null || true\n"
                "git push origin --delete \"$BRANCH\" 2>/dev/null || true\n"
            ),
            shell=True,
            capture_stdout=False,
        ),
    )
    merge_git_cleanup.timeout(600)

    # ----------------------------------------------------------------
    # move_to_done
    # ----------------------------------------------------------------
    move_to_done = wf.task(
        "move_to_done",
        gh.project_item_set_status(
            item_id=wf.input.board_item_id,
            board=resolve_board_ids.output,
            status="Done",
            on_error="warn",
        ),
    )

    # ----------------------------------------------------------------
    # success (terminal: success)
    # ----------------------------------------------------------------
    success = wf.task(
        "success",
        command(
            "echo \"Job is completed based on opencode output\" | tee /dev/tty",
            shell=True,
            capture_stdout=False,
        ),
    )
    success._terminal = "success"

    # ----------------------------------------------------------------
    # retry_implementation
    # ----------------------------------------------------------------
    retry_implementation = wf.task(
        "retry_implementation",
        command(
            "echo \"Job failed (status != COMPLETED), retrying implement_spec task...\"",
            shell=True,
            capture_stdout=False,
        ),
    )
    retry_implementation.repeat_at_most(3)

    # ----------------------------------------------------------------
    # Wire transitions
    # ----------------------------------------------------------------
    ensure_clean_main.then(resolve_board_ids)
    resolve_board_ids.then(move_to_in_progress)
    move_to_in_progress.then(prepare_spec_paths)
    prepare_spec_paths.then(read_develop_spec_path)
    read_develop_spec_path.then(create_branch)
    create_branch.then(load_spec)
    load_spec.then(implement_spec)

    implement_spec.then(
        run_tests,
        when=expr('tasks.implement_spec.output.signal == "complete"'),
    ).then(retry_implementation)

    run_tests.then(
        snapshot_commit,
        when=expr('contains(tasks.run_tests.output.stdout, "TEST_STATUS: passed")'),
    ).then(fix_test_failures)

    fix_test_failures.then(run_tests)

    snapshot_commit.then(
        get_diff,
        when=expr(
            'contains(tasks.snapshot_commit.output.stdout, "COMMIT_STATUS: success") || '
            'contains(tasks.snapshot_commit.output.stdout, "COMMIT_STATUS: skipped")'
        ),
    ).then(fix_snapshot_precommit)

    fix_snapshot_precommit.then(snapshot_commit)
    get_diff.then(validation_preflight)

    validation_preflight.then(
        validate_against_spec,
        when=expr('contains(tasks.validation_preflight.output.stdout, "VALIDATION_PREFLIGHT: ok")'),
    ).then(fail_validation_oversized)

    validate_against_spec.then(
        analyze_code_health,
        when=expr('tasks.validate_against_spec.output.signal == "valid"'),
    ).then(
        load_validation_feedback,
        when=expr('tasks.validate_against_spec.output.signal == "invalid"'),
    )

    load_validation_feedback.then(implement_feedback)
    implement_feedback.then(run_tests)

    analyze_code_health.then(
        fix_code_health,
        when=expr('contains(tasks.analyze_code_health.output.stdout, "{")'),
    ).then(git_stage)

    fix_code_health.then(git_stage)

    git_stage.then(
        no_changes_done,
        when=expr('contains(tasks.git_stage.output.stdout, "NO_CHANGES")'),
    ).then(
        git_push,
        when=expr('contains(tasks.git_stage.output.stdout, "COMMITTED_ONLY")'),
    ).then(
        git_commit,
        when=expr('contains(tasks.git_stage.output.stdout, "HAS_CHANGES")'),
    )

    git_commit.then(
        git_push,
        when=expr(
            'contains(tasks.git_commit.output.stdout, "COMMIT_STATUS: success") || '
            'contains(tasks.git_commit.output.stdout, "COMMIT_STATUS: skipped")'
        ),
    ).then(fix_final_precommit)

    fix_final_precommit.then(git_commit)
    git_push.then(gh_create_pr)
    gh_create_pr.then(move_to_in_review)

    move_to_in_review.then(
        poll_pr,
        when=expr('triggers.skip_gh_pr_approve == "true"'),
    ).then(
        gh_approve_pr,
        when=expr('triggers.skip_gh_pr_approve != "true"'),
    )

    gh_approve_pr.then(poll_pr)

    poll_pr.then(
        merge_git_cleanup,
        when=expr('tasks.poll_pr.output.state == "MERGED"'),
    ).then(
        move_to_ready_on_close,
        when=expr('tasks.poll_pr.output.state == "CLOSED"'),
    ).then(
        sleep_merge_wait,
        when=expr('tasks.poll_pr.output.state == "OPEN"'),
    ).then(
        sleep_merge_unknown,
        when=expr(
            'tasks.poll_pr.output.state != "OPEN" && tasks.poll_pr.output.state != "MERGED" '
            '&& tasks.poll_pr.output.state != "CLOSED"'
        ),
    )

    sleep_merge_wait.then(poll_pr)
    sleep_merge_unknown.then(poll_pr)
    move_to_ready_on_close.then(fail_pr_closed)
    merge_git_cleanup.then(move_to_done)
    move_to_done.then(success)
    retry_implementation.then(implement_spec)

    return wf


def test_develop_compiles() -> None:
    """The develop workflow compiles without error."""
    wf = build_develop()
    with warnings.catch_warnings(record=True):
        warnings.simplefilter("always")
        yaml_str = wf.to_yaml()

    assert yaml_str, "to_yaml() returned empty string"


def test_develop_validates() -> None:
    """Compiled YAML passes newton workflow validate."""
    wf = build_develop()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        yaml_str = wf.to_yaml()

    ok, output = validate_with_newton(yaml_str)
    assert ok, f"newton validate failed:\n{output}\n\nYAML:\n{yaml_str}"


def test_develop_semantic_equality() -> None:
    """Compiled YAML is semantically equal to the conformance fixture."""
    wf = build_develop()
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        yaml_str = wf.to_yaml()

    compiled = yaml.safe_load(yaml_str)
    expected = load_fixture("develop")

    # version, mode
    assert compiled.get("version") == expected.get("version"), "version mismatch"
    assert compiled.get("mode") == expected.get("mode"), "mode mismatch"

    # metadata
    compiled_name = (compiled.get("metadata") or {}).get("name")
    expected_name = (expected.get("metadata") or {}).get("name")
    assert compiled_name == expected_name, f"metadata.name: {compiled_name!r} != {expected_name!r}"

    # settings
    compiled_settings = normalize_yaml(compiled["workflow"]["settings"])
    expected_settings = normalize_yaml(expected["workflow"]["settings"])
    for key in ["entry_task", "parallel_limit", "continue_on_error", "max_task_iterations",
                "max_workflow_iterations", "default_engine"]:
        assert compiled_settings.get(key) == expected_settings.get(key), (
            f"settings.{key}: {compiled_settings.get(key)!r} != {expected_settings.get(key)!r}"
        )

    # tasks
    ok, diff = tasks_semantic_equal(
        compiled["workflow"]["tasks"],
        expected["workflow"]["tasks"],
    )
    assert ok, f"Task semantic mismatch:\n{diff}"
