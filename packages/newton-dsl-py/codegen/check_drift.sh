#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TMP=$(mktemp -d)
cp "$SCRIPT_DIR/../src/newton/_generated/ir.py" "$TMP/ir.py.orig"
cp "$SCRIPT_DIR/../src/newton/_generated/output_schemas.py" "$TMP/output_schemas.py.orig"
bash "$SCRIPT_DIR/generate.sh"
DRIFT=0
if ! diff -q "$SCRIPT_DIR/../src/newton/_generated/ir.py" "$TMP/ir.py.orig" > /dev/null 2>&1; then
  echo "ERROR: _generated/ir.py has drifted. Run codegen/generate.sh to update."
  DRIFT=1
fi
if ! diff -q "$SCRIPT_DIR/../src/newton/_generated/output_schemas.py" "$TMP/output_schemas.py.orig" > /dev/null 2>&1; then
  echo "ERROR: _generated/output_schemas.py has drifted. Run codegen/generate.sh to update."
  DRIFT=1
fi
[ $DRIFT -eq 0 ] && echo "OK: _generated/ is up to date."
exit $DRIFT
