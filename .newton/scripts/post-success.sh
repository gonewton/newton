#!/bin/bash
set -euo pipefail

[[ -n "${NEWTON_PROJECT_ROOT:-}" && -n "${NEWTON_TASK_ID:-}" ]] || { echo "NEWTON_PROJECT_ROOT and NEWTON_TASK_ID are required." >&2; exit 1; }

STATE_DIR="${NEWTON_STATE_DIR:-$NEWTON_PROJECT_ROOT/.newton/tasks/$NEWTON_TASK_ID/state}"
CONTROL_FILE="${NEWTON_CONTROL_FILE:-$STATE_DIR/newton_control.json}"
BRANCH_NAME="${NEWTON_BRANCH_NAME:-feature/${NEWTON_TASK_ID//_/\-}}"
WS_ROOT="${NEWTON_WS_ROOT:-}"
if [[ -z "$WS_ROOT" ]]; then
  echo "NEWTON_WS_ROOT is required." >&2
  exit 1
fi

NEWTON_CODER_CMD="${NEWTON_CODER_CMD:-$WS_ROOT/.newton/scripts/coder.sh}"
WAIT_SCRIPT="$WS_ROOT/.newton/scripts/wait-pr-merged.sh"
mkdir -p "$STATE_DIR"

BASE_BRANCH="${NEWTON_BASE_BRANCH:-}"
if [[ -z "$BASE_BRANCH" ]]; then
  BASE_BRANCH=$(git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null | sed 's|^refs/remotes/origin/||') || true
  BASE_BRANCH="${BASE_BRANCH:-main}"
fi

notify_ailoop() {
  local msg="$1"
  if command -v ailoop &>/dev/null; then
    ailoop say "$msg"
  fi
  echo "$msg"
}

ensure_branch() {
  if ! git rev-parse --abbrev-ref HEAD >/dev/null 2>&1 || [[ "$(git rev-parse --abbrev-ref HEAD)" != "$BRANCH_NAME" ]]; then
    git checkout "$BRANCH_NAME" >/dev/null 2>&1 || git checkout -b "$BRANCH_NAME"
  fi
}

commit_and_create_pr() {
  ensure_branch
  local prompt
  prompt="Newton run completed. Base branch is: $BASE_BRANCH. Current branch: $BRANCH_NAME.\nYou MUST: 1) Stage all changes (git add), 2) Commit with a clear message derived from the goal and changes, 3) Push to origin, 4) Run: gh pr create --base $BASE_BRANCH with a good title and body. Do all steps now."
  echo "$prompt" | "$NEWTON_CODER_CMD" -y
}

write_summary_ok() {
  local out_dir="$NEWTON_PROJECT_ROOT/.newton/tasks/$NEWTON_TASK_ID/output"
  mkdir -p "$out_dir"
  echo '{"status":"ok"}' > "$out_dir/summary.json"
}

cleanup_branch() {
  if [[ -n "$BASE_BRANCH" ]] && [[ "$(git rev-parse --abbrev-ref HEAD 2>/dev/null)" != "$BASE_BRANCH" ]]; then
    git checkout "$BASE_BRANCH"
  fi
}

delete_branch_if_exists() {
  git rev-parse --verify "$BRANCH_NAME" >/dev/null 2>&1 && git branch -D "$BRANCH_NAME" >/dev/null 2>&1 || true
}

run_newton() {
  export NEWTON_CONTROL_FILE="$CONTROL_FILE"
  echo "Running Newton loop inside post-success" >&2
  newton run "$NEWTON_PROJECT_ROOT" --goal-file "$NEWTON_GOAL_FILE" --verbose \
    --evaluator-cmd "$NEWTON_PROJECT_ROOT/.newton/scripts/evaluator.sh" \
    --advisor-cmd "$NEWTON_PROJECT_ROOT/.newton/scripts/advisor.sh" \
    --executor-cmd "$WS_ROOT/.newton/scripts/executor.sh" \
    --max-iterations 5 --max-time 3600 2>&1 | tee -a "$STATE_DIR/newton_run.log"
  return "${PIPESTATUS[0]}"
}

FEEDBACK_FILE="$STATE_DIR/pr_feedback.txt"
LAST_REVIEW_ID_FILE="$STATE_DIR/pr_last_review_id.txt"

cd "$NEWTON_PROJECT_ROOT"
ensure_branch
commit_and_create_pr
write_summary_ok

PR_URL=$(gh pr view "$BRANCH_NAME" --json url -q '.url' 2>/dev/null) || true
if [[ -z "$PR_URL" ]]; then
  notify_ailoop "Newton optimization completed but no PR found for branch $BRANCH_NAME"
  cleanup_branch
  delete_branch_if_exists
  exit 1
fi

while true; do
  "$WAIT_SCRIPT" "$BRANCH_NAME" 60 "$FEEDBACK_FILE" "$LAST_REVIEW_ID_FILE"
  WAIT_EXIT=$?
  if [[ $WAIT_EXIT -eq 0 ]]; then
    notify_ailoop "Newton optimization completed. PR merged for branch $BRANCH_NAME"
    cleanup_branch
    delete_branch_if_exists
    exit 0
  fi
  if [[ $WAIT_EXIT -eq 1 ]]; then
    notify_ailoop "Newton optimization completed. PR closed without merge for branch $BRANCH_NAME"
    cleanup_branch
    delete_branch_if_exists
    exit 0
  fi
  FEEDBACK=$(cat "$FEEDBACK_FILE" 2>/dev/null || true)
  cat > "$STATE_DIR/context.md" <<'FEEDBACK'
# User Feedback

$FEEDBACK
FEEDBACK
  echo "PENDING" > "$STATE_DIR/promise.txt"
  run_newton
  commit_and_create_pr
  [[ -f "${FEEDBACK_FILE}.review_id" ]] && cp "${FEEDBACK_FILE}.review_id" "$LAST_REVIEW_ID_FILE"
done
