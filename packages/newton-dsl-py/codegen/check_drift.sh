#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TMP=$(mktemp -d)
cp "$SCRIPT_DIR/../src/newton/_generated/ir.py" "$TMP/ir.py.orig"
bash "$SCRIPT_DIR/generate.sh"
if ! diff -q "$SCRIPT_DIR/../src/newton/_generated/ir.py" "$TMP/ir.py.orig" > /dev/null 2>&1; then
  echo "ERROR: _generated/ir.py has drifted from the schema. Run codegen/generate.sh to update."
  exit 1
fi
echo "OK: _generated/ir.py is up to date."
