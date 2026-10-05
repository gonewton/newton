#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 ]]; then
  echo "usage: $0 WORKSPACE PROJECT EVIDENCE_DIR [harness options...]" >&2
  exit 2
fi

workspace=$(realpath "$1")
project=$2
evidence=$(realpath -m "$3")
shift 3
mkdir -p "$evidence"

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
image=${NEWTON_OPTIMIZE_LIVE_IMAGE:-newton-optimize-live:local}
pi_agent_dir=${PI_AGENT_DIR:-$HOME/.pi/agent}
docker_environment=()
if [[ -n "${NEWTON_LIVE_ENV_FILE:-}" ]]; then
  docker_environment+=(--env-file "$(realpath "$NEWTON_LIVE_ENV_FILE")")
fi

if [[ ! -f "$pi_agent_dir/models.json" ]]; then
  echo "not exercised: Pi active registry is missing at $pi_agent_dir/models.json" >&2
  exit 3
fi

if [[ "${NEWTON_LIVE_SKIP_BUILD:-0}" != 1 ]]; then
  docker build --network host -f "$repo_root/scripts/Dockerfile.optimize-live" -t "$image" "$repo_root"
fi
if [[ -n "${OPENAI_API_KEY:-}" ]]; then
  docker_environment+=(--env OPENAI_API_KEY)
fi
if [[ -n "${NEWTON_LIVE_GATEWAY_HOST:-}" ]]; then
  docker_environment+=(--add-host "${NEWTON_LIVE_GATEWAY_HOST}:${NEWTON_LIVE_GATEWAY_IP:?set gateway IP}")
fi

docker run --rm --network host \
  --user "$(id -u):$(id -g)" \
  "${docker_environment[@]}" \
  --mount "type=bind,src=$workspace,dst=/workspace" \
  --mount "type=bind,src=$evidence,dst=/evidence" \
  --mount "type=bind,src=$(realpath "$pi_agent_dir"),dst=/pi-config,readonly" \
  "$image" /workspace "$project" newton --evidence-dir /evidence/trial "$@"
