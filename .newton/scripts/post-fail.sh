#!/bin/bash
set -euo pipefail

if [[ -z "${NEWTON_PROJECT_ROOT:-}" || -z "${NEWTON_TASK_ID:-}" ]]; then
  echo "Missing NEWTON_PROJECT_ROOT or NEWTON_TASK_ID; skipping post-fail hook." >&2
  exit 0
fi

BRANCH_NAME="${NEWTON_BRANCH_NAME:-feature/${NEWTON_TASK_ID//_/\-}}"
BASE_BRANCH="${NEWTON_BASE_BRANCH:-}"
cd "$NEWTON_PROJECT_ROOT"

if [[ -z "$BASE_BRANCH" ]]; then
  BASE_BRANCH=$(git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null | sed 's|^refs/remotes/origin/||') || true
  BASE_BRANCH="${BASE_BRANCH:-main}"
fi

cleanup_branch() {
  if [[ -n "$BASE_BRANCH" ]] && [[ "$(git rev-parse --abbrev-ref HEAD 2>/dev/null)" != "$BASE_BRANCH" ]]; then
    git checkout "$BASE_BRANCH"
  fi
}

delete_branch() {
  git rev-parse --verify "$BRANCH_NAME" >/dev/null 2>&1 && git branch -D "$BRANCH_NAME" >/dev/null 2>&1 || true
}

notify_failure() {
  local msg="Newton run failed for task $NEWTON_TASK_ID (branch $BRANCH_NAME)"
  if command -v ailoop &>/dev/null; then
    ailoop say "$msg"
  fi
  echo "$msg"
}

cleanup_branch
delete_branch
notify_failure
exit 1
