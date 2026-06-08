#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR/.."

uv run --with datamodel-code-generator \
  datamodel-codegen \
  --input "$(realpath ../../workflow-schema/workflow.schema.json)" \
  --input-file-type jsonschema \
  --output src/newton/_generated/ir.py \
  --output-model-type pydantic_v2.BaseModel \
  --target-python-version 3.11 \
  --use-schema-description \
  --reuse-model
echo "Regenerated src/newton/_generated/ir.py"

python3 - << 'PYEOF'
import json, pathlib
schema = json.loads(pathlib.Path("../../workflow-schema/output_schemas.json").read_text())
entries = []
for op in sorted(schema):
    props = schema[op].get("properties", {})
    if props:
        entries.append(f'    "{op}": {sorted(props)!r},')
lines = [
    "# AUTO-GENERATED — do not edit by hand.",
    "# Regenerate with: bash codegen/generate.sh",
    "OUTPUT_SCHEMAS: dict[str, list[str]] = {",
] + entries + ["}", ""]
pathlib.Path("src/newton/_generated/output_schemas.py").write_text("\n".join(lines))
print("Regenerated src/newton/_generated/output_schemas.py")
PYEOF
