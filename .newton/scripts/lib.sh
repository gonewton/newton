#!/bin/bash
# Shared helpers for Newton scripts.

# Outputs the trimmed value for key in key=value config file. Lines with # are comments.
read_conf_value() {
  local file="$1"
  local key="$2"
  [[ ! -f "$file" ]] && return
  while IFS= read -r line; do
    line="${line%%#*}"
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    [[ -z "$line" || "$line" != *"="* ]] && continue
    local k="${line%%=*}"
    k="${k%"${k##*[![:space:]]}"}"
    local v="${line#*=}"
    v="${v#"${v%%[![:space:]]*}"}"
    v="${v%"${v##*[![:space:]]}"}"
    if [[ "$k" = "$key" ]]; then
      echo "$v"
      return
    fi
  done < "$file"
}
