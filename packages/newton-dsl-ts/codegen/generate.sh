#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

SCHEMA="$(cd "$(dirname "$0")/../.." && pwd)/workflow-schema/workflow.schema.json"
# Fallback to repo root if in worktree
if [ ! -f "$SCHEMA" ]; then
  REPO_ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel 2>/dev/null || echo "")"
  if [ -n "$REPO_ROOT" ]; then
    SCHEMA="$REPO_ROOT/packages/workflow-schema/workflow.schema.json"
  fi
fi

if [ ! -f "$SCHEMA" ]; then
  echo "ERROR: workflow.schema.json not found at $SCHEMA" >&2
  exit 1
fi

pnpm dlx json-schema-to-typescript \
  "$SCHEMA" \
  --no-additionalProperties \
  --unreachableDefinitions \
  > src/generated/ir.ts

echo "Generated src/generated/ir.ts from $SCHEMA"
