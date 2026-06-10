#!/usr/bin/env bash
# Deterministic structural grader for the widgets-cli fixture.
# Contract: print a Newton Assessment JSON to stdout; do NOT self-persist.
# Usage: seed-grader.sh <scope_id> <repo_path>
set -euo pipefail

SCOPE_ID="${1:-}"
REPO_PATH="${2:-.}"

if [ -z "$SCOPE_ID" ] || [ -z "$REPO_PATH" ]; then
  echo "Usage: seed-grader.sh <scope_id> <repo_path>" >&2
  exit 1
fi

INVENTORY="${REPO_PATH}/src/widgets/inventory.py"
FORMATTING="${REPO_PATH}/src/widgets/formatting.py"
REPORT="${REPO_PATH}/src/widgets/report.py"
CLI="${REPO_PATH}/src/widgets/cli.py"

# ── Detect issues ────────────────────────────────────────────────────────────
issue_a=false  # god-function: process() line count > 60
issue_b=false  # duplicate print_table in both formatting.py and report.py
issue_c=false  # dead stub validate_unused
issue_d=false  # substring dispatch cmd.find(

if [ -f "$INVENTORY" ]; then
  # Issue A: count non-blank lines inside the process() function body
  process_lines=$(python3 - "$INVENTORY" <<'PY'
import sys, re
path = sys.argv[1]
with open(path) as f:
    lines = f.readlines()
in_process = False
count = 0
indent_level = None
for line in lines:
    stripped = line.rstrip()
    if re.match(r'^def process\(', stripped):
        in_process = True
        count = 0
        continue
    if in_process:
        if stripped == '' or stripped.startswith('#'):
            continue
        indent = len(line) - len(line.lstrip())
        if indent_level is None and indent > 0:
            indent_level = indent
        if indent_level is not None and indent < indent_level and stripped.startswith('def '):
            break
        count += 1
print(count)
PY
  )
  if [ "$process_lines" -gt 40 ]; then
    issue_a=true
  fi

  # Issue C: dead stub validate_unused
  if grep -q "^def validate_unused" "$INVENTORY" 2>/dev/null; then
    issue_c=true
  fi
fi

# Issue B: duplicate print_table
if [ -f "$FORMATTING" ] && [ -f "$REPORT" ]; then
  fmt_has=$(grep -c "^def print_table" "$FORMATTING" 2>/dev/null || echo 0)
  rpt_has=$(grep -c "^def print_table" "$REPORT" 2>/dev/null || echo 0)
  if [ "$fmt_has" -gt 0 ] && [ "$rpt_has" -gt 0 ]; then
    issue_b=true
  fi
fi

# Issue D: substring dispatch
if [ -f "$CLI" ]; then
  if grep -q 'cmd\.find(' "$CLI" 2>/dev/null; then
    issue_d=true
  fi
fi

# ── Score computation ────────────────────────────────────────────────────────
PENALTY=0
[ "$issue_a" = "true" ] && PENALTY=$((PENALTY + 12))
[ "$issue_b" = "true" ] && PENALTY=$((PENALTY + 10))
[ "$issue_c" = "true" ] && PENALTY=$((PENALTY + 8))
[ "$issue_d" = "true" ] && PENALTY=$((PENALTY + 10))
SCORE=$((100 - PENALTY))

# ── Build Assessment JSON ────────────────────────────────────────────────────
NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
GRADER_ID="seed-grader"
RUN_ID=$(python3 -c "import uuid; print(str(uuid.uuid4()))")

build_observation() {
  local id="$1" dimension="$2" title="$3" action="$4" severity="$5" location="$6"
  printf '{"id":"%s","dimension":"%s","title":"%s","why_it_matters":"Reduces maintainability.","recommended_action":"%s","severity":"%s","confidence":0.95,"location":%s}' \
    "$id" "$dimension" "$title" "$action" "$severity" "$location"
}

OBSERVATIONS="["
SEP=""

[ "$issue_a" = "true" ] && {
  OBSERVATIONS="${OBSERVATIONS}${SEP}$(build_observation "obs-a" "file_decomposition" \
    "process() is a god-function mixing validation, state, and hooks" \
    "Split into focused helpers: _validate_add, _do_add, _validate_remove, _do_remove" \
    "high" '{"file":"src/widgets/inventory.py","function":"process"}')"
  SEP=","
}

[ "$issue_b" = "true" ] && {
  OBSERVATIONS="${OBSERVATIONS}${SEP}$(build_observation "obs-b" "canonical_placement" \
    "print_table duplicated verbatim in formatting.py and report.py" \
    "Remove print_table from report.py; import from formatting.py (canonical location)" \
    "medium" '{"file":"src/widgets/report.py","function":"print_table"}')"
  SEP=","
}

[ "$issue_c" = "true" ] && {
  OBSERVATIONS="${OBSERVATIONS}${SEP}$(build_observation "obs-c" "abstraction_economy" \
    "validate_unused() is a dead stub never called anywhere" \
    "Delete validate_unused from inventory.py" \
    "low" '{"file":"src/widgets/inventory.py","function":"validate_unused"}')"
  SEP=","
}

[ "$issue_d" = "true" ] && {
  OBSERVATIONS="${OBSERVATIONS}${SEP}$(build_observation "obs-d" "branching_discipline" \
    "Command dispatch uses substring matching (cmd.find) instead of exact-match dict" \
    "Replace if/elif cmd.find chain with a dict-based dispatch table" \
    "medium" '{"file":"src/widgets/cli.py","function":"main"}')"
  SEP=","
}

OBSERVATIONS="${OBSERVATIONS}]"

# Per-dimension scores
DIM_SCORES="["
DIM_SEP=""
for dim in file_decomposition canonical_placement abstraction_economy branching_discipline; do
  penalty=0
  case "$dim" in
    file_decomposition)  [ "$issue_a" = "true" ] && penalty=12 ;;
    canonical_placement) [ "$issue_b" = "true" ] && penalty=10 ;;
    abstraction_economy) [ "$issue_c" = "true" ] && penalty=8  ;;
    branching_discipline)[ "$issue_d" = "true" ] && penalty=10 ;;
  esac
  dim_score=$((100 - penalty))
  DIM_SCORES="${DIM_SCORES}${DIM_SEP}{\"dimension\":\"${dim}\",\"score\":${dim_score}}"
  DIM_SEP=","
done
DIM_SCORES="${DIM_SCORES}]"

python3 - <<PYEOF
import json, sys
assessment = {
    "id": "$RUN_ID",
    "source": "$GRADER_ID",
    "scope": "repo",
    "scope_id": "$SCOPE_ID",
    "grader": "$GRADER_ID",
    "overall_score": $SCORE,
    "evaluated_at": "$NOW",
    "observations": $OBSERVATIONS,
    "dimension_scores": $DIM_SCORES,
    "summary": f"widgets-cli maintainability score: {$SCORE}/100"
}
print(json.dumps(assessment, indent=2))
PYEOF
