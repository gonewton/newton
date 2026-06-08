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

# Generate output_schemas.ts from the companion output_schemas.json
OUTPUT_SCHEMAS_JSON="$(dirname "$SCHEMA")/output_schemas.json"
if [ ! -f "$OUTPUT_SCHEMAS_JSON" ]; then
  echo "ERROR: output_schemas.json not found at $OUTPUT_SCHEMAS_JSON" >&2
  exit 1
fi

python3 - "$OUTPUT_SCHEMAS_JSON" << 'PYEOF'
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
pathlib.Path("src/generated/output_schemas.ts").write_text("\n".join(lines))
print("Generated src/generated/output_schemas.ts")
PYEOF
