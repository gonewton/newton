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

TMPFILE=$(mktemp /tmp/ir_check_XXXXXX.ts)
TMPFILE2=$(mktemp /tmp/output_schemas_check_XXXXXX.ts)
trap "rm -f $TMPFILE $TMPFILE2" EXIT

pnpm dlx json-schema-to-typescript \
  "$SCHEMA" \
  --no-additionalProperties \
  --unreachableDefinitions \
  > "$TMPFILE" 2>/dev/null

OUTPUT_SCHEMAS_JSON="$(dirname "$SCHEMA")/output_schemas.json"
python3 - "$OUTPUT_SCHEMAS_JSON" > "$TMPFILE2" << 'PYEOF'
import json, pathlib, sys
schema = json.loads(pathlib.Path(sys.argv[1]).read_text())
entries = []
for op in sorted(schema):
    props = schema[op].get("properties", {})
    if props:
        fields = "[" + ", ".join(f'"{f}"' for f in sorted(props)) + "]"
        entries.append(f'  {op}: {fields},')
lines = [
    "// AUTO-GENERATED — do not edit by hand.",
    "// Regenerate with: bash codegen/generate.sh",
    "export const OUTPUT_SCHEMAS: Record<string, string[]> = {",
] + entries + ["};", ""]
print("\n".join(lines), end="")
PYEOF

DRIFT=0
if ! diff -u src/generated/ir.ts "$TMPFILE"; then
  echo ""
  echo "DRIFT DETECTED: src/generated/ir.ts is out of date." >&2
  echo "Run: bash codegen/generate.sh" >&2
  DRIFT=1
fi
if ! diff -u src/generated/output_schemas.ts "$TMPFILE2"; then
  echo ""
  echo "DRIFT DETECTED: src/generated/output_schemas.ts is out of date." >&2
  echo "Run: bash codegen/generate.sh" >&2
  DRIFT=1
fi
[ $DRIFT -eq 0 ] && echo "OK: src/generated/ is up to date with the schema."
exit $DRIFT
