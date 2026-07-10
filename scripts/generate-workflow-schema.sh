#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

# Regenerate the committed workflow-schema artifacts from the operator
# descriptor registry (ADR-0014). CI diffs these against the checked-in
# files so the schema can never drift from what the Rust registry exports
# (spec 074, P2).
cargo run -q -p newton-cli --bin newton -- schema export --pretty \
  --out packages/workflow-schema/workflow.schema.json
cargo run -q -p newton-cli --bin newton -- schema export --outputs --pretty \
  --out packages/workflow-schema/output_schemas.json
