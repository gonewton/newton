#!/usr/bin/env bash
# develop workflow (see workflow_path in project .conf): AgentOperator models are set in that YAML.
# Convention: engine claude_code -> model sonnet; engine opencode -> model zai-coding-plan/glm-5.
# Newton applies settings.model_stylesheet only when a task omits params.model (default there is sonnet).
set -e
. ".newton/scripts/newton-project-root.sh"

usage() {
  echo "Usage: develop <project_id>                 # pick next Ready board item and run workflow" >&2
  echo "       develop <project_id> resume <uuid>    # resume a workflow from checkpoint" >&2
}

project_id="${1:?Usage: develop <project_id> [resume <execution-id>]}"
project_root="$(resolve_project_root "$project_id")"

config_file="$NEWTON_CONFIG_DIR/${project_id}.conf"
# shellcheck disable=SC1090
. "$config_file"

[[ -n "${workflow_path:-}" ]]     || { echo "workflow_path is not set in $config_file" >&2; exit 1; }
[[ -n "${gh_project_owner:-}" ]]  || { echo "gh_project_owner is not set in $config_file" >&2; exit 1; }
[[ -n "${gh_project_number:-}" ]] || { echo "gh_project_number is not set in $config_file" >&2; exit 1; }

if [[ "${2:-}" == "resume" ]]; then
  execution_id="${3:?Usage: develop <project_id> resume <execution-id>}"
  export GH_PROJECT_OWNER="$gh_project_owner"
  export GH_PROJECT_NUMBER="$gh_project_number"
  export RUST_LOG=debug
  exec newton resume --execution-id "$execution_id" \
    --workspace "$project_root" \
    --allow-workflow-change
fi

if [[ -n "${2:-}" ]]; then
  usage
  exit 1
fi

item_json=$(gh project item-list "$gh_project_number" \
  --owner "$gh_project_owner" \
  --query "status:Ready" \
  --format json \
  --limit 1 | jq '.items[0] // empty')

[[ -n "$item_json" ]] || { echo "No items with status Ready on the board" >&2; exit 1; }

item_title=$(echo "$item_json" | jq -r '.title')
item_id=$(echo "$item_json" | jq -r '.id')
item_body=$(echo "$item_json" | jq -r '.content.body')

spec_path="/tmp/${item_title}.md"
echo "$item_body" > "$spec_path"

echo "Picked: $item_title (ID: $item_id)"
echo "Spec:   $spec_path"

export GH_PROJECT_OWNER="$gh_project_owner"
export GH_PROJECT_NUMBER="$gh_project_number"
export RUST_LOG=debug
exec newton run "$workflow_path" \
  --workspace "$project_root" \
  --arg "prompt=$spec_path" \
  --arg "board_item_id=$item_id" \
  --verbose
