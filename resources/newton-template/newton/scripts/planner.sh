#!/usr/bin/env bash
set -e
. ".newton/scripts/newton-project-root.sh"

project_id="${1:?Usage: planner <project_id>}"
project_root="$(resolve_project_root "$project_id")"

config_file="$NEWTON_CONFIG_DIR/${project_id}.conf"
# shellcheck disable=SC1090
. "$config_file"

[[ -n "${gh_project_owner:-}" ]]  || { echo "gh_project_owner is not set in $config_file" >&2; exit 1; }
[[ -n "${gh_project_number:-}" ]] || { echo "gh_project_number is not set in $config_file" >&2; exit 1; }

WORKFLOW=".newton/workflows/planner.yaml"

item_json=$(gh project item-list "$gh_project_number" \
  --owner "$gh_project_owner" \
  --query "status:Draft" \
  --format json \
  --limit 1 | jq '.items[0] // empty')

[[ -n "$item_json" ]] || { echo "No items with status Draft on the board" >&2; exit 1; }

item_title=$(echo "$item_json"        | jq -r '.title')
item_id=$(echo "$item_json"          | jq -r '.id')
item_content_id=$(echo "$item_json"  | jq -r '.content.id')
item_issue_number=$(echo "$item_json" | jq -r '.content.number // empty')
item_body=$(echo "$item_json"        | jq -r '.content.body')

spec_path="/tmp/${item_title}.md"
echo "$item_body" > "$spec_path"

echo "Picked: $item_title (ID: $item_id)"
echo "Spec:   $spec_path"

export GH_PROJECT_OWNER="$gh_project_owner"
export GH_PROJECT_NUMBER="$gh_project_number"
# --server must match newton serve bind (default 127.0.0.1:8080).
exec newton run "$WORKFLOW" \
  --workspace "$project_root" \
  --arg "prompt=$spec_path" \
  --arg "output_path=$spec_path" \
  --arg "board_item_id=$item_id" \
  --arg "board_content_id=$item_content_id" \
  --arg "board_issue_number=${item_issue_number}" \
  --arg "board_item_title=$item_title" \
  --verbose \
  --server http://127.0.0.1:8080
