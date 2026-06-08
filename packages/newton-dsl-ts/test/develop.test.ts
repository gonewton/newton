/**
 * Round-trip test for develop.yaml.
 *
 * Authors the workflow in TypeScript using @newton/dsl, compiles to YAML,
 * and asserts semantic equality with the conformance fixture.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import * as Y from "yaml";
import {
  Workflow,
  agent,
  command,
  gh,
  expr,
  AmbientRef,
} from "../src/index.js";
import { loadFixture, tasksSemanticEqual, normalizeSettings } from "./helpers.js";

function buildDevelop(): Workflow {
  const artifactStorage = {
    base_path: ".newton/artifacts",
    max_inline_bytes: 4194304,
    max_artifact_bytes: 104857600,
    max_total_bytes: 1073741824,
    retention_hours: 168,
    cleanup_policy: "lru",
  };

  const wf = new Workflow("rust-workflow", {
    description:
      "Implement from a repo-local spec path. Prefer raw_spec_path + board_issue_number + " +
      "board_item_title (develop.sh); workflow copies into tmp/<issue>-<slug>.md before git work.",
    defaultEngine: "opencode",
    parallelLimit: 1,
    maxTimeSeconds: 604800,
    continueOnError: false,
    maxTaskIterations: 15000,
    maxWorkflowIterations: 500,
    allowShell: true,
    artifactStorage,
  });

  wf.inputs({
    prompt: "",
    raw_spec_path: "",
    board_item_id: "",
    board_issue_number: "",
    board_item_title: "",
    skip_gh_pr_approve: "true",
  });

  wf.expects(
    "develop_primary_engine",
    "develop_primary_model",
    "develop_secondary_engine",
    "develop_secondary_model"
  );

  wf.setContext({
    preamble:
      "Implement the following spec in the target source code target.\n" +
      "After you have applied the changes and ./scripts/run-tests.sh completes with no errors, " +
      "print exactly <status>COMPLETED</status> in your final output.",
    codescene_preamble:
      "Fix code health issues reported by CodeScene. Work in the target source code base; " +
      "run ./scripts/run-tests.sh to verify. Do not require a COMPLETED marker.",
    precommit_fix_preamble:
      "The pre-commit hook rejected a commit. The full hook output is below.\n" +
      "Fix all reported issues in the codebase. Run ./scripts/run-tests.sh to verify.\n" +
      "Do not attempt to commit; only fix the code.",
    validation_preamble:
      "You are a reviewer. You will be given the spec path (read the spec file in the workspace) " +
      "and a bounded diff vs main (stat plus truncated patch).\n" +
      "Determine whether the provided diff fully implements the given spec.\n" +
      "Reply with exactly one of <status>VALID</status> or <status>INVALID</status>.\n" +
      "If INVALID, include a single block <feedback>...</feedback> with clear, actionable feedback.",
  });

  const primaryEngine = new AmbientRef("develop_primary_engine");
  const primaryModel = new AmbientRef("develop_primary_model");
  const secondaryEngine = new AmbientRef("develop_secondary_engine");
  const secondaryModel = new AmbientRef("develop_secondary_model");

  // ----------------------------------------------------------------
  // ensure_clean_main
  // ----------------------------------------------------------------
  const ensureCleanMain = wf.task(
    "ensure_clean_main",
    command({
      cmd:
        "set -e\n" +
        'git diff --quiet || { echo "Precondition failed: unstaged changes. Commit or stash before develop."; exit 1; }\n' +
        'git diff --cached --quiet || { echo "Precondition failed: staged changes. Commit or stash before develop."; exit 1; }\n' +
        "git fetch origin\n" +
        "git checkout main\n" +
        "git pull --rebase origin main\n",
      shell: true,
      captureStdout: false,
    }),
    { name: "Ensure clean worktree" }
  );

  // ----------------------------------------------------------------
  // resolve_board_ids
  // ----------------------------------------------------------------
  const resolveBoardIds = wf.task(
    "resolve_board_ids",
    gh.projectResolveBoard({
      owner: wf.env("GH_PROJECT_OWNER"),
      projectNumber: wf.env("GH_PROJECT_NUMBER"),
    }),
    { name: "Pick next Issue" }
  );

  // ----------------------------------------------------------------
  // move_to_in_progress
  // ----------------------------------------------------------------
  const moveToInProgress = wf.task(
    "move_to_in_progress",
    gh.projectItemSetStatus({
      itemId: wf.input.board_item_id,
      board: resolveBoardIds.output,
      status: "In progress",
      onError: "warn",
    }),
    { name: "Move Issue to In Progress" }
  );

  // ----------------------------------------------------------------
  // prepare_spec_paths
  // ----------------------------------------------------------------
  const prepareSpecPaths = wf.task(
    "prepare_spec_paths",
    command({
      cmd:
        "set -eo pipefail\n" +
        "mkdir -p .newton/plan tmp\n" +
        "slugify() {\n" +
        "  s=$(printf '%s' \"${TITLE:-}\" | iconv -f utf-8 -t ascii//TRANSLIT 2>/dev/null || printf '%s' \"${TITLE:-}\")\n" +
        "  s=$(printf '%s' \"$s\" | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9]+/-/g; s/^-+|-+$//g')\n" +
        "  [ -z \"$s\" ] && s=\"spec\"\n" +
        "  printf '%s' \"$s\" | cut -c1-60 | sed -E 's/-+$//'\n" +
        "}\n" +
        "if [ -n \"$RAW_SPEC\" ] && [ -f \"$RAW_SPEC\" ]; then\n" +
        "  slug=$(slugify)\n" +
        "  OUT=\"tmp/${ISSUE}-${slug}.md\"\n" +
        "  if [ \"$RAW_SPEC\" != \"$OUT\" ]; then\n" +
        "    cp \"$RAW_SPEC\" \"$OUT\"\n" +
        "  fi\n" +
        "else\n" +
        "  OUT=\"$LEGACY_PROMPT\"\n" +
        "  if [ -z \"$OUT\" ] || [ ! -f \"$OUT\" ]; then\n" +
        '    echo "prepare_spec_paths: set raw_spec_path to an existing file or pass prompt with a readable path"\n' +
        "    exit 1\n" +
        "  fi\n" +
        "fi\n" +
        "printf '%s\\n' \"$OUT\" > .newton/plan/.develop-prompt-path\n" +
        'echo "spec_path=$OUT"\n',
      shell: true,
      captureStdout: true,
      env: {
        ISSUE: wf.input.board_issue_number,
        TITLE: wf.input.board_item_title,
        RAW_SPEC: wf.input.raw_spec_path,
        LEGACY_PROMPT: wf.input.prompt,
      },
    })
  );

  // ----------------------------------------------------------------
  // read_develop_spec_path
  // ----------------------------------------------------------------
  const readDevelopSpecPath = wf.task(
    "read_develop_spec_path",
    command({
      cmd: "set -eo pipefail\nprintf '%s' \"$(cat .newton/plan/.develop-prompt-path)\"\n",
      shell: true,
      captureStdout: true,
    })
  );

  // ----------------------------------------------------------------
  // create_branch
  // ----------------------------------------------------------------
  const createBranch = wf.task(
    "create_branch",
    command({
      cmd:
        "BRANCH_NAME=$(basename \"$PROMPT_PATH\" | sed 's/\\.[^.]*$//')\n" +
        "git checkout -b \"feature/$BRANCH_NAME\"\n",
      shell: true,
      env: { PROMPT_PATH: readDevelopSpecPath.out.stdout },
      captureStdout: false,
    }),
    { name: "Create Branch" }
  );

  // ----------------------------------------------------------------
  // load_spec
  // ----------------------------------------------------------------
  const loadSpec = wf.task(
    "load_spec",
    command({
      cmd: "set -e\ncat \"$SPEC_PATH\"\n",
      shell: true,
      env: { SPEC_PATH: readDevelopSpecPath.out.stdout },
      captureStdout: true,
      captureStderr: true,
    })
  );

  // ----------------------------------------------------------------
  // implement_spec
  // ----------------------------------------------------------------
  const implementSpec = wf.task(
    "implement_spec",
    agent({
      engine: primaryEngine,
      model: primaryModel,
      prompt: expr(
        'context.preamble + "\\n\\nSpec path: " + tasks.read_develop_spec_path.output.stdout + ' +
          '"\\n\\nSpec content:\\n" + tasks.load_spec.output.stdout'
      ),
      signals: { complete: "<status>COMPLETED</status>" },
    })
  );
  implementSpec.repeatAtMost(3);

  // ----------------------------------------------------------------
  // run_tests
  // ----------------------------------------------------------------
  const runTests = wf.task(
    "run_tests",
    command({
      cmd:
        "OUTPUT=$(./scripts/run-tests.sh 2>&1)\n" +
        "EXIT=$?\n" +
        'echo "$OUTPUT"\n' +
        '[ $EXIT -eq 0 ] && echo "TEST_STATUS: passed" || echo "TEST_STATUS: failed"\n',
      shell: true,
      captureStdout: true,
      captureStderr: true,
    })
  );
  runTests.repeatAtMost(60);

  // ----------------------------------------------------------------
  // fix_test_failures
  // ----------------------------------------------------------------
  const fixTestFailures = wf.task(
    "fix_test_failures",
    agent({
      engine: secondaryEngine,
      model: secondaryModel,
      prompt: expr(
        'context.preamble + "\\n\\nSpec path: " + tasks.read_develop_spec_path.output.stdout + ' +
          '"\\n\\nSpec content:\\n" + tasks.load_spec.output.stdout + ' +
          '"\\n\\nTests failed. Fix the issues and re-run ./scripts/run-tests.sh:\\n" + ' +
          "tasks.run_tests.output.stdout"
      ),
    })
  );
  fixTestFailures.repeatAtMost(3);

  // ----------------------------------------------------------------
  // snapshot_commit
  // ----------------------------------------------------------------
  const snapshotCommit = wf.task(
    "snapshot_commit",
    command({
      cmd:
        "SPEC_BASENAME=$(basename \"$SPEC_PATH\" | sed 's/\\.[^.]*$//')\n" +
        "git add -A\n" +
        "git diff --cached --name-only | { grep -E '^test_results\\.' || true; } | xargs -r git reset --\n" +
        "if git diff --cached --quiet; then\n" +
        '  echo "COMMIT_STATUS: skipped"\n' +
        "  exit 0\n" +
        "fi\n" +
        'OUTPUT=$(git commit -m "chore(develop): $SPEC_BASENAME" 2>&1)\n' +
        "EXIT=$?\n" +
        'echo "$OUTPUT"\n' +
        '[ $EXIT -eq 0 ] && echo "COMMIT_STATUS: success" || echo "COMMIT_STATUS: failed"\n',
      shell: true,
      env: { SPEC_PATH: readDevelopSpecPath.out.stdout },
      captureStdout: true,
      captureStderr: true,
    })
  );
  snapshotCommit.repeatAtMost(90);

  // ----------------------------------------------------------------
  // fix_snapshot_precommit
  // ----------------------------------------------------------------
  const fixSnapshotPrecommit = wf.task(
    "fix_snapshot_precommit",
    agent({
      engine: secondaryEngine,
      model: secondaryModel,
      prompt: expr(
        'context.precommit_fix_preamble + "\\n\\nPre-commit hook output:\\n" + ' +
          "tasks.snapshot_commit.output.stdout"
      ),
    })
  );
  fixSnapshotPrecommit.repeatAtMost(3);

  // ----------------------------------------------------------------
  // get_diff
  // ----------------------------------------------------------------
  const getDiff = wf.task(
    "get_diff",
    command({
      cmd:
        "set -e\n" +
        'echo "=== diff stat ==="\n' +
        "git diff --stat main...HEAD\n" +
        'echo ""\n' +
        'echo "=== diff (bounded, max 256KiB) ==="\n' +
        "git diff -U3 main...HEAD | head -c 262144\n" +
        "DIFF_BYTES=$(git diff main...HEAD | wc -c | tr -d ' ')\n" +
        'echo ""\n' +
        'echo "=== diff_bytes=$DIFF_BYTES ==="\n',
      shell: true,
      captureStdout: true,
    })
  );
  getDiff.repeatAtMost(60);

  // ----------------------------------------------------------------
  // validation_preflight
  // ----------------------------------------------------------------
  const validationPreflight = wf.task(
    "validation_preflight",
    command({
      cmd:
        "set -e\n" +
        "SPEC_BYTES=$(wc -c < \"$SPEC_PATH\" | tr -d ' ')\n" +
        "DIFF_BYTES=$(git diff main...HEAD | wc -c | tr -d ' ')\n" +
        "PREAMBLE_BUDGET=8192\n" +
        "TOTAL=$((SPEC_BYTES + DIFF_BYTES + PREAMBLE_BUDGET))\n" +
        "MAX_BYTES=400000\n" +
        'echo "spec_bytes=$SPEC_BYTES diff_bytes=$DIFF_BYTES total_estimate=$TOTAL max_bytes=$MAX_BYTES"\n' +
        'if [ "$TOTAL" -gt "$MAX_BYTES" ]; then\n' +
        '  echo "VALIDATION_PREFLIGHT: oversize"\n' +
        "  exit 1\n" +
        "fi\n" +
        'echo "VALIDATION_PREFLIGHT: ok"\n',
      shell: true,
      captureStdout: true,
      env: { SPEC_PATH: readDevelopSpecPath.out.stdout },
    })
  );

  // ----------------------------------------------------------------
  // fail_validation_oversized
  // ----------------------------------------------------------------
  const failValidationOversized = wf.task(
    "fail_validation_oversized",
    command({
      cmd:
        'echo "develop: validation preflight failed — spec+diff exceeds safe prompt budget." >&2\n' +
        'echo "See validation_preflight output for byte counts. Split the change or spec before re-running." >&2\n' +
        "exit 1\n",
      shell: true,
      captureStdout: false,
    })
  );
  failValidationOversized._terminal = "failure";

  // ----------------------------------------------------------------
  // validate_against_spec
  // ----------------------------------------------------------------
  const validateAgainstSpec = wf.task(
    "validate_against_spec",
    agent({
      engine: primaryEngine,
      model: primaryModel,
      requireSignal: true,
      prompt: expr(
        'context.validation_preamble + "\\n\\nSpec path (read in workspace): " + ' +
          "tasks.read_develop_spec_path.output.stdout + " +
          '"\\n\\nDiff vs main (stat + bounded diff):\\n" + tasks.get_diff.output.stdout'
      ),
      signals: {
        valid: "<status>VALID</status>",
        invalid: "<status>INVALID</status>",
      },
    })
  );
  validateAgainstSpec.repeatAtMost(3);

  // ----------------------------------------------------------------
  // load_validation_feedback
  // ----------------------------------------------------------------
  const loadValidationFeedback = wf.task(
    "load_validation_feedback",
    command({
      cmd: 'cat "$VALIDATION_STDOUT_ARTIFACT"',
      shell: true,
      captureStdout: true,
      env: { VALIDATION_STDOUT_ARTIFACT: validateAgainstSpec.out.stdout_artifact },
    })
  );

  // ----------------------------------------------------------------
  // implement_feedback
  // ----------------------------------------------------------------
  const implementFeedback = wf.task(
    "implement_feedback",
    agent({
      engine: primaryEngine,
      model: primaryModel,
      prompt: expr(
        'context.preamble + "\\n\\nSpec path: " + tasks.read_develop_spec_path.output.stdout + ' +
          '"\\n\\nSpec content:\\n" + tasks.load_spec.output.stdout + ' +
          '"\\n\\nValidation feedback (address and re-run tests):\\n" + ' +
          "tasks.load_validation_feedback.output.stdout"
      ),
      signals: { complete: "<status>COMPLETED</status>" },
    })
  );
  implementFeedback.repeatAtMost(5);

  // ----------------------------------------------------------------
  // analyze_code_health
  // ----------------------------------------------------------------
  const analyzeCodeHealth = wf.task(
    "analyze_code_health",
    command({ cmd: "cs delta --output-format json", shell: true, captureStdout: true })
  );

  // ----------------------------------------------------------------
  // fix_code_health
  // ----------------------------------------------------------------
  const fixCodeHealth = wf.task(
    "fix_code_health",
    agent({
      engine: secondaryEngine,
      model: secondaryModel,
      prompt: expr(
        'context.codescene_preamble + "\\n\\nCodeScene delta report (JSON):\\n" + ' +
          "tasks.analyze_code_health.output.stdout"
      ),
    })
  );

  // ----------------------------------------------------------------
  // git_stage
  // ----------------------------------------------------------------
  const gitStage = wf.task(
    "git_stage",
    command({
      cmd:
        "set -e\n" +
        "git add -A\n" +
        "git diff --cached --name-only | { grep -E '^test_results\\.' || true; } | xargs -r git reset --\n" +
        "if git diff --cached --quiet; then\n" +
        "  if git diff --quiet main...HEAD; then\n" +
        '    echo "NO_CHANGES"\n' +
        "  else\n" +
        '    echo "COMMITTED_ONLY"\n' +
        "  fi\n" +
        "  exit 0\n" +
        "fi\n" +
        'echo "HAS_CHANGES"\n',
      shell: true,
      captureStdout: true,
    })
  );

  // ----------------------------------------------------------------
  // no_changes_done (terminal: success)
  // ----------------------------------------------------------------
  const noChangesDone = wf.task(
    "no_changes_done",
    command({ cmd: 'echo "No changes to commit; skipping PR."', shell: true, captureStdout: false })
  );
  noChangesDone._terminal = "success";

  // ----------------------------------------------------------------
  // git_commit
  // ----------------------------------------------------------------
  const gitCommit = wf.task(
    "git_commit",
    command({
      cmd:
        'MSG="feat: implement $(basename "$PROMPT_PATH" .md 2>/dev/null || echo "spec")"\n' +
        "if git diff --cached --quiet; then\n" +
        '  echo "No changes to commit"\n' +
        '  echo "COMMIT_STATUS: skipped"\n' +
        "  exit 0\n" +
        "fi\n" +
        'OUTPUT=$(git commit -m "$MSG" 2>&1)\n' +
        "EXIT=$?\n" +
        'echo "$OUTPUT"\n' +
        '[ $EXIT -eq 0 ] && echo "COMMIT_STATUS: success" || echo "COMMIT_STATUS: failed"\n',
      shell: true,
      env: { PROMPT_PATH: readDevelopSpecPath.out.stdout },
      captureStdout: true,
      captureStderr: true,
    })
  );

  // ----------------------------------------------------------------
  // fix_final_precommit
  // ----------------------------------------------------------------
  const fixFinalPrecommit = wf.task(
    "fix_final_precommit",
    agent({
      engine: secondaryEngine,
      model: secondaryModel,
      prompt: expr(
        'context.precommit_fix_preamble + "\\n\\nPre-commit hook output:\\n" + ' +
          "tasks.git_commit.output.stdout"
      ),
    })
  );
  fixFinalPrecommit.repeatAtMost(3);

  // ----------------------------------------------------------------
  // git_push
  // ----------------------------------------------------------------
  const gitPush = wf.task(
    "git_push",
    command({
      cmd:
        "for i in 1 2 3; do\n" +
        "  git push -u origin HEAD && exit 0\n" +
        "  sleep 5\n" +
        "done\n" +
        'echo "git push failed after 3 attempts"; exit 1\n',
      shell: true,
      captureStdout: true,
      captureStderr: true,
    })
  );

  // ----------------------------------------------------------------
  // gh_create_pr
  // ----------------------------------------------------------------
  const ghCreatePr = wf.task(
    "gh_create_pr",
    gh.prCreate({
      base: "main",
      title: expr('"feat: implement " + file_stem(tasks.read_develop_spec_path.output.stdout)'),
      body: "Implements spec. Merge with squash.",
      retryCount: 3,
      retryDelayMs: 5000,
    })
  );

  // ----------------------------------------------------------------
  // move_to_in_review
  // ----------------------------------------------------------------
  const moveToInReview = wf.task(
    "move_to_in_review",
    gh.projectItemSetStatus({
      itemId: wf.input.board_item_id,
      board: resolveBoardIds.output,
      status: "In review",
      onError: "warn",
    })
  );

  // ----------------------------------------------------------------
  // gh_approve_pr
  // ----------------------------------------------------------------
  const ghApprovePr = wf.task(
    "gh_approve_pr",
    gh.prApprove({ prNumber: ghCreatePr.out.pr_number })
  );

  // ----------------------------------------------------------------
  // poll_pr
  // ----------------------------------------------------------------
  const pollPr = wf.task(
    "poll_pr",
    gh.prView({ pr: ghCreatePr.out.pr_number }),
    { name: "Wait for PR Acceptance" }
  );
  pollPr.repeatAtMost(15000);
  pollPr.timeout(120);

  // ----------------------------------------------------------------
  // sleep_merge_wait
  // ----------------------------------------------------------------
  const sleepMergeWait = wf.task(
    "sleep_merge_wait",
    command({ cmd: "sleep 60", shell: true, captureStdout: false })
  );
  sleepMergeWait.repeatAtMost(15000);

  // ----------------------------------------------------------------
  // sleep_merge_unknown
  // ----------------------------------------------------------------
  const sleepMergeUnknown = wf.task(
    "sleep_merge_unknown",
    command({ cmd: "echo \"Unexpected PR state; waiting 30s\"; sleep 30", shell: true, captureStdout: false })
  );
  sleepMergeUnknown.repeatAtMost(15000);

  // ----------------------------------------------------------------
  // move_to_ready_on_close
  // ----------------------------------------------------------------
  const moveToReadyOnClose = wf.task(
    "move_to_ready_on_close",
    gh.projectItemSetStatus({
      itemId: wf.input.board_item_id,
      board: resolveBoardIds.output,
      status: "Ready",
      onError: "warn",
    })
  );

  // ----------------------------------------------------------------
  // fail_pr_closed (terminal: failure)
  // ----------------------------------------------------------------
  const failPrClosed = wf.task(
    "fail_pr_closed",
    command({ cmd: 'echo "PR closed without merge" >&2; exit 1', shell: true, captureStdout: false })
  );
  failPrClosed._terminal = "failure";

  // ----------------------------------------------------------------
  // merge_git_cleanup
  // ----------------------------------------------------------------
  const mergeGitCleanup = wf.task(
    "merge_git_cleanup",
    command({
      cmd:
        "set -e\n" +
        "if [ -f .git/MERGE_HEAD ] || [ -d .git/rebase-merge ] || [ -d .git/rebase-apply ] || [ -f .git/CHERRY_PICK_HEAD ]; then\n" +
        '  echo "merge_git_cleanup: unfinished merge/rebase/cherry-pick (cannot checkout main). Resolve conflicts or abort, then rerun." >&2\n' +
        "  git status >&2 || true\n" +
        "  exit 1\n" +
        "fi\n" +
        "BRANCH=$(git branch --show-current)\n" +
        "git checkout main\n" +
        "git pull --rebase origin main\n" +
        "git branch -d \"$BRANCH\" 2>/dev/null || true\n" +
        "git push origin --delete \"$BRANCH\" 2>/dev/null || true\n",
      shell: true,
      captureStdout: false,
    })
  );
  mergeGitCleanup.timeout(600);

  // ----------------------------------------------------------------
  // move_to_done
  // ----------------------------------------------------------------
  const moveToDone = wf.task(
    "move_to_done",
    gh.projectItemSetStatus({
      itemId: wf.input.board_item_id,
      board: resolveBoardIds.output,
      status: "Done",
      onError: "warn",
    })
  );

  // ----------------------------------------------------------------
  // success (terminal: success)
  // ----------------------------------------------------------------
  const success = wf.task(
    "success",
    command({
      cmd: 'echo "Job is completed based on opencode output" | tee /dev/tty',
      shell: true,
      captureStdout: false,
    })
  );
  success._terminal = "success";

  // ----------------------------------------------------------------
  // retry_implementation
  // ----------------------------------------------------------------
  const retryImplementation = wf.task(
    "retry_implementation",
    command({
      cmd: 'echo "Job failed (status != COMPLETED), retrying implement_spec task..."',
      shell: true,
      captureStdout: false,
    })
  );
  retryImplementation.repeatAtMost(3);

  // ----------------------------------------------------------------
  // Wire transitions
  // ----------------------------------------------------------------
  ensureCleanMain.then(resolveBoardIds);
  resolveBoardIds.then(moveToInProgress);
  moveToInProgress.then(prepareSpecPaths);
  prepareSpecPaths.then(readDevelopSpecPath);
  readDevelopSpecPath.then(createBranch);
  createBranch.then(loadSpec);
  loadSpec.then(implementSpec);

  implementSpec
    .then(runTests, { when: expr('tasks.implement_spec.output.signal == "complete"') })
    .then(retryImplementation);

  runTests
    .then(snapshotCommit, {
      when: expr('contains(tasks.run_tests.output.stdout, "TEST_STATUS: passed")'),
    })
    .then(fixTestFailures);

  fixTestFailures.then(runTests);

  snapshotCommit
    .then(getDiff, {
      when: expr(
        'contains(tasks.snapshot_commit.output.stdout, "COMMIT_STATUS: success") || ' +
          'contains(tasks.snapshot_commit.output.stdout, "COMMIT_STATUS: skipped")'
      ),
    })
    .then(fixSnapshotPrecommit);

  fixSnapshotPrecommit.then(snapshotCommit);
  getDiff.then(validationPreflight);

  validationPreflight
    .then(validateAgainstSpec, {
      when: expr('contains(tasks.validation_preflight.output.stdout, "VALIDATION_PREFLIGHT: ok")'),
    })
    .then(failValidationOversized);

  validateAgainstSpec
    .then(analyzeCodeHealth, {
      when: expr('tasks.validate_against_spec.output.signal == "valid"'),
    })
    .then(loadValidationFeedback, {
      when: expr('tasks.validate_against_spec.output.signal == "invalid"'),
    });

  loadValidationFeedback.then(implementFeedback);
  implementFeedback.then(runTests);

  analyzeCodeHealth
    .then(fixCodeHealth, {
      when: expr('contains(tasks.analyze_code_health.output.stdout, "{")'),
    })
    .then(gitStage);

  fixCodeHealth.then(gitStage);

  gitStage
    .then(noChangesDone, {
      when: expr('contains(tasks.git_stage.output.stdout, "NO_CHANGES")'),
    })
    .then(gitPush, {
      when: expr('contains(tasks.git_stage.output.stdout, "COMMITTED_ONLY")'),
    })
    .then(gitCommit, {
      when: expr('contains(tasks.git_stage.output.stdout, "HAS_CHANGES")'),
    });

  gitCommit
    .then(gitPush, {
      when: expr(
        'contains(tasks.git_commit.output.stdout, "COMMIT_STATUS: success") || ' +
          'contains(tasks.git_commit.output.stdout, "COMMIT_STATUS: skipped")'
      ),
    })
    .then(fixFinalPrecommit);

  fixFinalPrecommit.then(gitCommit);
  gitPush.then(ghCreatePr);
  ghCreatePr.then(moveToInReview);

  moveToInReview
    .then(pollPr, { when: expr('triggers.skip_gh_pr_approve == "true"') })
    .then(ghApprovePr, { when: expr('triggers.skip_gh_pr_approve != "true"') });

  ghApprovePr.then(pollPr);

  pollPr
    .then(mergeGitCleanup, { when: expr('tasks.poll_pr.output.state == "MERGED"') })
    .then(moveToReadyOnClose, { when: expr('tasks.poll_pr.output.state == "CLOSED"') })
    .then(sleepMergeWait, { when: expr('tasks.poll_pr.output.state == "OPEN"') })
    .then(sleepMergeUnknown, {
      when: expr(
        'tasks.poll_pr.output.state != "OPEN" && tasks.poll_pr.output.state != "MERGED" ' +
          '&& tasks.poll_pr.output.state != "CLOSED"'
      ),
    });

  sleepMergeWait.then(pollPr);
  sleepMergeUnknown.then(pollPr);
  moveToReadyOnClose.then(failPrClosed);
  mergeGitCleanup.then(moveToDone);
  moveToDone.then(success);
  retryImplementation.then(implementSpec);

  return wf;
}

describe("develop round-trip", () => {
  beforeEach(() => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("compiles without throwing CompilerError", () => {
    const wf = buildDevelop();
    const yaml = wf.toYaml();
    expect(yaml).toBeTruthy();
  });

  it("is semantically equal to the conformance fixture", () => {
    const wf = buildDevelop();
    const yamlStr = wf.toYaml();
    const compiled = Y.parse(yamlStr) as Record<string, unknown>;
    const expected = loadFixture("develop");

    expect(compiled.version).toBe(expected.version);
    expect(compiled.mode).toBe(expected.mode);

    const compiledMeta = compiled.metadata as Record<string, unknown>;
    const expectedMeta = expected.metadata as Record<string, unknown>;
    expect(compiledMeta?.name).toBe(expectedMeta?.name);

    const cWf = compiled.workflow as Record<string, unknown>;
    const eWf = expected.workflow as Record<string, unknown>;
    const cSettings = normalizeSettings(cWf.settings);
    const eSettings = normalizeSettings(eWf.settings);

    for (const key of [
      "entry_task",
      "parallel_limit",
      "continue_on_error",
      "max_task_iterations",
      "max_workflow_iterations",
      "default_engine",
    ]) {
      expect(cSettings[key]).toEqual(eSettings[key]);
    }

    const { equal, diff } = tasksSemanticEqual(
      cWf.tasks as unknown[],
      eWf.tasks as unknown[]
    );
    expect(equal, `Task semantic mismatch:\n${diff}`).toBe(true);
  });
});
