#!/bin/bash
# Wait for a PR to be merged on remote. Polls until state is MERGED, CLOSED, or feedback.
# Usage: wait-pr-merged.sh [branch_or_pr_number] [poll_seconds] [feedback_file] [last_processed_review_id_file]
# If first arg omitted, uses current branch. Default poll: 60s.
# Optional feedback_file: when PR is OPEN and latest review is CHANGES_REQUESTED or COMMENTED with non-empty body,
#   the review body is written to this file and the script exits 2.
# Optional last_processed_review_id_file: only exit 2 when the latest such review id differs.
# Exit codes: 0 = merged, 1 = closed/no PR/error, 2 = feedback (body written to feedback_file).
set -e

TARGET="${1:-$(git rev-parse --abbrev-ref HEAD 2>/dev/null)}"
POLL_SEC="${2:-60}"
FEEDBACK_FILE="${3:-}"
LAST_REVIEW_ID_FILE="${4:-}"

require_gh() {
  if ! command -v gh &>/dev/null; then
    echo "gh CLI is required. Install it and run 'gh auth login'." >&2
    exit 1
  fi
}

handle_merged() {
  echo "PR merged. Updating local main..."
  local base
  base=$(git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null | sed 's|^refs/remotes/origin/||' || echo "main")
  git checkout "$base" && git pull
  echo "Done."
  exit 0
}

# If OPEN and latest review is CHANGES_REQUESTED or COMMENTED with body, write feedback and exit 2. Otherwise return.
open_maybe_emit_feedback() {
  [[ -z "$FEEDBACK_FILE" ]] && return
  local latest
  latest=$(gh pr view "$TARGET" --json reviews -q '.reviews | sort_by(.submittedAt) | last' 2>/dev/null) || true
  [[ -z "$latest" || "$latest" = "null" ]] && return
  local state body id last_id
  state=$(echo "$latest" | jq -r '.state // ""')
  body=$(echo "$latest" | jq -r '.body // ""')
  id=$(echo "$latest" | jq -r '.id // ""')
  [[ "$state" != "CHANGES_REQUESTED" && "$state" != "COMMENTED" || -z "$body" ]] && return
  last_id=""
  if [[ -n "$LAST_REVIEW_ID_FILE" && -f "$LAST_REVIEW_ID_FILE" ]]; then
    last_id=$(cat "$LAST_REVIEW_ID_FILE" 2>/dev/null | tr -d '\n\r')
  fi
  [[ "$id" = "$last_id" ]] && return
  echo "$body" > "$FEEDBACK_FILE"
  echo "$id" > "${FEEDBACK_FILE}.review_id"
  echo "Review feedback received. Feedback written to $FEEDBACK_FILE. Exiting for another round."
  exit 2
}

# --- main

require_gh
if [[ -z "$TARGET" || "$TARGET" = "HEAD" ]]; then
  echo "Usage: wait-pr-merged.sh <branch_name_or_pr_number> [poll_seconds] [feedback_file] [last_processed_review_id_file]" >&2
  exit 1
fi

PR_URL=$(gh pr view "$TARGET" --json url -q '.url' 2>/dev/null) || true
if [[ -n "$PR_URL" ]]; then
  echo "Waiting for PR to be merged. Polling every ${POLL_SEC}s."
  echo "PR: $PR_URL"
else
  echo "Waiting for PR (branch: $TARGET) to be merged on remote. Polling every ${POLL_SEC}s..."
fi

while true; do
  STATE=$(gh pr view "$TARGET" --json state -q '.state' 2>/dev/null) || true
  case "$STATE" in
    MERGED)  handle_merged ;;
    CLOSED)
      echo "PR was closed without merge. Exiting." >&2
      exit 1
      ;;
    OPEN)
      PR_URL=$(gh pr view "$TARGET" --json url -q '.url' 2>/dev/null) || true
      [[ -n "$PR_URL" ]] && echo "PR: $PR_URL"
      open_maybe_emit_feedback
      echo "PR still open. Checking again in ${POLL_SEC}s..."
      sleep "$POLL_SEC"
      ;;
    *)
      echo "Could not get PR status for $TARGET (gh pr view failed or no PR found)." >&2
      exit 1
      ;;
  esac
done
