#!/usr/bin/env bash
set -e

NEWTON_CONFIG_DIR="${NEWTON_CONFIG_DIR:-.newton/configs}"

resolve_project_root() {
  local project_id="$1"
  local config_file="$NEWTON_CONFIG_DIR/${project_id}.conf"

  if [[ -z "$project_id" ]]; then
    echo "project_id is required" >&2
    return 1
  fi

  if [[ ! -f "$config_file" ]]; then
    echo "Config not found for project_id '$project_id' at $config_file" >&2
    return 1
  fi

  # shellcheck disable=SC1090
  . "$config_file"

  if [[ -z "$project_root" ]]; then
    echo "project_root is not set in $config_file" >&2
    return 1
  fi

  printf '%s\n' "$project_root"
}

