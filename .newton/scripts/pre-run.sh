#!/bin/bash
set -euo pipefail

if [[ -z "${NEWTON_PROJECT_ROOT:-}" || -z "${NEWTON_TASK_ID:-}" ]]; then
  echo "NEWTON_PROJECT_ROOT and NEWTON_TASK_ID are required." >&2
  exit 1
fi

BRANCH_NAME="${NEWTON_BRANCH_NAME:-}"
if [[ -z "$BRANCH_NAME" ]]; then
  BRANCH_NAME="feature/${NEWTON_TASK_ID//_/\-}"
fi
RESUME=0
if [[ "${NEWTON_RESUME:-0}" =~ ^(1|true)$ ]]; then
  RESUME=1
fi

STATE_DIR="${NEWTON_STATE_DIR:-$NEWTON_PROJECT_ROOT/.newton/tasks/$NEWTON_TASK_ID/state}"
PROJECT_STATE_DIR="$NEWTON_PROJECT_ROOT/.newton/state"

clear_control_file() {
  if [[ -n "${NEWTON_CONTROL_FILE:-}" ]]; then
    rm -f "$NEWTON_CONTROL_FILE"
  fi
}

stash_changes() {
  if git diff --quiet && git diff --cached --quiet; then
    return 1
  fi
  git stash push -u -m "newton-pre-checkout"
  return 0
}

require_goal_file() {
  if [[ -z "${NEWTON_GOAL_FILE:-}" ]]; then
    echo "NEWTON_GOAL_FILE must be set so pre-run can reference the plan." >&2
    exit 1
  fi
}

require_goal_file
clear_control_file
cd "$NEWTON_PROJECT_ROOT"

STASHED=0
if stash_changes; then
  STASHED=1
fi

mkdir -p "$STATE_DIR" "$PROJECT_STATE_DIR"
echo "Preparing feature branch $BRANCH_NAME (resume=$RESUME)"

if ! git checkout main 2>/dev/null; then
  git checkout refs/heads/main >/dev/null 2>&1
fi
if ! git pull --ff-only; then
  echo "git pull failed" >&2
  exit 1
fi

if git rev-parse --verify "$BRANCH_NAME" >/dev/null 2>&1; then
  if [[ $RESUME -eq 1 ]]; then
    git checkout "$BRANCH_NAME"
  else
    git branch -D "$BRANCH_NAME"
    git checkout -b "$BRANCH_NAME"
  fi
else
  git checkout -b "$BRANCH_NAME"
fi

if [[ $STASHED -eq 1 && $RESUME -eq 1 ]]; then
  git stash pop || true
fi
