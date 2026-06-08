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
