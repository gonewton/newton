#!/usr/bin/env bash
# Ingest dk review findings into Newton as Opportunity records + optional EvalRun/Grades.
# Usage: ingest-dk-review.sh -w <workspace> -s <scope-id> [-p <path>] [--scope <type>] [--with-evalrun]
set -euo pipefail

WORKSPACE=""
SCOPE_ID=""
SCOPE_TYPE="repo"
DK_PATH="."
WITH_EVALRUN=false

usage() {
    echo "Usage: $0 -w <workspace> -s <scope-id> [-p <path>] [--scope <type>] [--with-evalrun]"
    echo ""
    echo "  -w <workspace>       Path to the Newton workspace"
    echo "  -s <scope-id>        Newton repo or component ID (UUID)"
    echo "  -p <path>            Path to pass to dk review (default: current directory)"
    echo "  --scope <type>       Scope type for EvalRun: repo, component, product, module (default: repo)"
    echo "  --with-evalrun       Write one EvalRun + per-dimension Grades via 'newton data' (no server required)"
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -w) WORKSPACE="$2"; shift 2 ;;
        -s) SCOPE_ID="$2"; shift 2 ;;
        -p) DK_PATH="$2"; shift 2 ;;
        --scope) SCOPE_TYPE="$2"; shift 2 ;;
        --with-evalrun) WITH_EVALRUN=true; shift ;;
        *) echo "Unknown argument: $1"; usage ;;
    esac
done

if [[ -z "$SCOPE_ID" ]]; then
    echo "Error: -s <scope-id> is required"
    usage
fi

if [[ "$WITH_EVALRUN" == true && -z "$WORKSPACE" ]]; then
    echo "Error: -w <workspace> is required when using --with-evalrun"
    usage
fi

if ! command -v jq &>/dev/null; then
    echo "Error: jq is required but not found in PATH"
    exit 1
fi

# Use a temp file to avoid any stdout pollution from dk
DK_TMPFILE=$(mktemp /tmp/dk-review-XXXXXX.json)
trap 'rm -f "$DK_TMPFILE"' EXIT

dk review "$DK_PATH" --output-format json --output-file "$DK_TMPFILE" 2>/dev/null || true

if [[ ! -s "$DK_TMPFILE" ]]; then
    echo "No findings from dk review for scope '$SCOPE_ID'"
    exit 0
fi

# Detect output shape and extract findings array
# dk --output-file produces: full review object {findings:[...], grades:{...}, ...}
DK_ROOT_TYPE=$(jq -r 'type' "$DK_TMPFILE" 2>/dev/null || echo "unknown")

if [[ "$DK_ROOT_TYPE" == "array" ]]; then
    DK_JSON=$(jq -c '.' "$DK_TMPFILE")
elif [[ "$DK_ROOT_TYPE" == "object" ]]; then
    DK_JSON=$(jq -c '.findings // []' "$DK_TMPFILE")
else
    echo "Error: unexpected dk output format (type=$DK_ROOT_TYPE)"
    exit 1
fi

FINDING_COUNT=$(echo "$DK_JSON" | jq 'length' 2>/dev/null || echo 0)
echo "Found $FINDING_COUNT finding(s) for scope '$SCOPE_ID'"

if [[ "$FINDING_COUNT" -eq 0 ]]; then
    echo "No findings to ingest."
    exit 0
fi

if [[ "$WITH_EVALRUN" != true ]]; then
    echo "$DK_JSON" | jq .
fi

if [[ "$WITH_EVALRUN" == true ]]; then
    if ! command -v newton &>/dev/null; then
        echo "Error: 'newton' CLI is required but not found in PATH"
        exit 1
    fi

    EVALUATED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    RUN_ID="evalrun.dk-review.${SCOPE_TYPE}.${SCOPE_ID}.${EVALUATED_AT}"

    # Build inline grades array: one grade per unique dimension, with kpiId mapped from dimension.
    GRADES_JSON=$(echo "$DK_JSON" | jq -c '
        def kpi_map: {
            "overall_code_health": "kpi.dk.code-health",
            "functionality":       "kpi.dk.code-health",
            "design":              "kpi.dk.design-quality",
            "change_scope":        "kpi.dk.design-quality",
            "tests":               "kpi.dk.test-quality",
            "complexity":          "kpi.dk.maintainability",
            "naming":              "kpi.dk.maintainability",
            "comments":            "kpi.dk.maintainability",
            "consistency":         "kpi.dk.maintainability",
            "documentation":       "kpi.dk.documentation",
            "cl_description":      "kpi.dk.documentation",
            "style":               "kpi.dk.documentation"
        };
        [
            group_by(.dimension // "general")[]
            | {
                dimension: (.[0].dimension // "general"),
                score: ([100 - (length * 10), 0] | max | [., 100] | min),
                kpiId: (.[0].dimension as $d | kpi_map[$d]),
                evidence: { findings: . }
              }
        ]
    ')

    GRADE_COUNT=$(echo "$GRADES_JSON" | jq 'length')

    RUN_PAYLOAD=$(jq -n \
        --arg id "$RUN_ID" \
        --arg source "dk-review" \
        --arg scope "$SCOPE_TYPE" \
        --arg scopeId "$SCOPE_ID" \
        --arg summary "dk review findings: ${FINDING_COUNT}" \
        --arg evaluatedAt "$EVALUATED_AT" \
        --argjson grades "$GRADES_JSON" \
        '{
            id: $id,
            source: $source,
            scope: $scope,
            scopeId: $scopeId,
            score: null,
            verdict: null,
            summary: $summary,
            evaluatedAt: $evaluatedAt,
            grades: $grades
        }')

    echo "[eval-run] creating $RUN_ID with $GRADE_COUNT inline grades"
    newton data post eval-run --workspace "$WORKSPACE" --body "$RUN_PAYLOAD" --json >/dev/null
fi

# POST each finding as an opportunity via newton data
if [[ -z "$WORKSPACE" ]]; then
    echo "Error: -w <workspace> is required to post opportunities"
    exit 1
fi

if ! command -v newton &>/dev/null; then
    echo "Error: 'newton' CLI is required but not found in PATH"
    exit 1
fi

# Determine opportunity scope field: repo or component
if [[ "$SCOPE_TYPE" == "repo" ]]; then
    SCOPE_FIELD="repo"
else
    SCOPE_FIELD="component"
fi

echo "$DK_JSON" | jq -c '.[]' | while read -r finding; do
    FINDING_ID=$(echo "$finding" | jq -r '.id // empty')
    FINDING_TITLE=$(echo "$finding" | jq -r '.observation // empty')
    FINDING_RISK=$(echo "$finding" | jq -r '.severity // "minor"')
    FINDING_RATIONALE=$(echo "$finding" | jq -r '.why_it_matters // empty')

    if [[ -z "$FINDING_ID" || -z "$FINDING_TITLE" ]]; then
        echo "[skip] finding missing id or observation"
        continue
    fi

    OPPORTUNITY_ID="dk-review.${SCOPE_ID}.${FINDING_ID}"

    PAYLOAD=$(jq -n \
        --arg id "$OPPORTUNITY_ID" \
        --arg title "$FINDING_TITLE" \
        --arg scopeField "$SCOPE_FIELD" \
        --arg scopeId "$SCOPE_ID" \
        --arg risk "$FINDING_RISK" \
        --argjson ev 0.0 \
        --arg rationale "$FINDING_RATIONALE" \
        '{
            id: $id,
            title: $title,
            origin: "dk-review",
            ($scopeField): $scopeId,
            risk: $risk,
            expectedValue: $ev,
            rationale: (if $rationale != "" then $rationale else null end)
        }')

    if newton data post opportunity --workspace "$WORKSPACE" --body "$PAYLOAD" --json >/dev/null 2>&1; then
        echo "[ok] $OPPORTUNITY_ID ($FINDING_RISK)"
    else
        echo "[error] $OPPORTUNITY_ID"
    fi
done
