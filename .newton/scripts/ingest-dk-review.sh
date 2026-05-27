#!/usr/bin/env bash
# Ingest dk review findings into Newton as Opportunity records + optional EvalRun/Grades.
# Usage: ingest-dk-review.sh -w <workspace> -s <scope-id> [--with-evalrun] [-u <server-url>]
set -euo pipefail

WORKSPACE=""
SCOPE_ID=""
WITH_EVALRUN=false
SERVER_URL="${NEWTON_SERVER_URL:-http://localhost:8080}"

usage() {
    echo "Usage: $0 -w <workspace> -s <scope-id> [--with-evalrun] [-u <server-url>]"
    echo ""
    echo "  -w <workspace>       Path to the Newton workspace"
    echo "  -s <scope-id>        Component scope id for dk review"
    echo "  --with-evalrun       Write one EvalRun + per-dimension Grades via 'newton data' (no server required)"
    echo "  -u <url>             Newton server base URL (default: \$NEWTON_SERVER_URL or http://localhost:8080)"
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -w) WORKSPACE="$2"; shift 2 ;;
        -s) SCOPE_ID="$2"; shift 2 ;;
        --with-evalrun) WITH_EVALRUN=true; shift ;;
        -u) SERVER_URL="$2"; shift 2 ;;
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

if ! command -v curl &>/dev/null; then
    echo "Error: curl is required but not found in PATH"
    exit 1
fi

# Run dk review and capture JSON output
DK_JSON=$(dk review --scope "$SCOPE_ID" --output-format json 2>/dev/null)

if [[ -z "$DK_JSON" ]]; then
    echo "No findings from dk review for scope '$SCOPE_ID'"
    exit 0
fi

FINDING_COUNT=$(echo "$DK_JSON" | jq 'length' 2>/dev/null || echo 0)
echo "Found $FINDING_COUNT finding(s) for scope '$SCOPE_ID'"

if [[ "$WITH_EVALRUN" != true ]]; then
    echo "$DK_JSON" | jq .
fi

if [[ "$WITH_EVALRUN" == true ]]; then
    if ! command -v newton &>/dev/null; then
        echo "Error: 'newton' CLI is required but not found in PATH"
        exit 1
    fi

    EVALUATED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    RUN_ID="evalrun.dk-review.component.${SCOPE_ID}.${EVALUATED_AT}"

    # Build inline grades array: one grade per unique dimension.
    GRADES_JSON=$(echo "$DK_JSON" | jq -c '
        [
            to_entries
            | group_by(.value.dimension // "general")[]
            | {
                dimension: (.[0].value.dimension // "general"),
                score: ([100 - (length * 10), 0] | max | [., 100] | min),
                kpiId: null,
                evidence: { findings: map(.value) }
              }
        ]
    ')

    RUN_PAYLOAD=$(jq -n \
        --arg id "$RUN_ID" \
        --arg source "dk-review" \
        --arg scope "component" \
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

    echo "[eval-run] creating $RUN_ID with ${#GRADES_JSON} inline grades"
    newton data post eval-run --workspace "$WORKSPACE" --body "$RUN_PAYLOAD" --json >/dev/null
fi

# POST each finding as an opportunity

echo "$DK_JSON" | jq -c '.[]' | while read -r finding; do
    FINDING_ID=$(echo "$finding" | jq -r '.id // empty')
    FINDING_TITLE=$(echo "$finding" | jq -r '.title // empty')
    FINDING_RISK=$(echo "$finding" | jq -r '.risk // "medium"')
    FINDING_RATIONALE=$(echo "$finding" | jq -r '.rationale // empty')

    if [[ -z "$FINDING_ID" || -z "$FINDING_TITLE" ]]; then
        echo "[skip] finding missing id or title"
        continue
    fi

    OPPORTUNITY_ID="dk-review.${SCOPE_ID}.${FINDING_ID}"

    PAYLOAD=$(jq -n \
        --arg id "$OPPORTUNITY_ID" \
        --arg title "$FINDING_TITLE" \
        --arg scope "$SCOPE_ID" \
        --arg risk "$FINDING_RISK" \
        --argjson ev 0.0 \
        --arg rationale "$FINDING_RATIONALE" \
        '{
            id: $id,
            title: $title,
            origin: "dk-review",
            component: $scope,
            risk: $risk,
            expectedValue: $ev,
            rationale: (if $rationale != "" then $rationale else null end)
        }')

    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" \
        -X POST \
        -H "Content-Type: application/json" \
        -d "$PAYLOAD" \
        "${SERVER_URL}/api/v1/opportunities")

    if [[ "$HTTP_CODE" == "201" ]]; then
        echo "[ok] $OPPORTUNITY_ID ($FINDING_RISK)"
    else
        echo "[error] $OPPORTUNITY_ID HTTP $HTTP_CODE"
    fi
done
