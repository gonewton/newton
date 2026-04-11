#!/usr/bin/env bash
set -e
. ".newton/scripts/newton-project-root.sh"

# Usage: documenter <project_id> [trigger_json_path]
# Runs newton documenter workflow against project_root from configs.
# Default trigger file: $project_root/.newton/documenter-trigger.json (if present).
# Trigger JSON keys: base_ref, allowlist (newline-separated paths), prompt, commit_docs, require_plan_approval.

project_id="${1:?Usage: documenter <project_id> [trigger_json_path]}"
shift || true
project_root="$(resolve_project_root "$project_id")"

config_file="$NEWTON_CONFIG_DIR/${project_id}.conf"
# shellcheck disable=SC1090
. "$config_file"

WORKFLOW="/home/sysuser/ws001/.newton/workflows/documenter.yaml"

trigger_json="${1:-}"
if [[ -z "$trigger_json" ]]; then
  default_trigger="$project_root/.newton/documenter-trigger.json"
  if [[ -f "$default_trigger" ]]; then
    trigger_json="$default_trigger"
  fi
fi

args=(newton run "$WORKFLOW" --workspace "$project_root" --verbose --server http://127.0.0.1:8080)
if [[ -n "$trigger_json" ]]; then
  args+=(--trigger-json "$trigger_json")
fi

# --server must match newton serve bind (default 127.0.0.1:8080).
exec "${args[@]}"
