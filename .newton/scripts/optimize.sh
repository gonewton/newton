#!/usr/bin/env bash
# optimize.sh — Newton optimization loop driver (spec 069)
# Usage: optimize.sh <project_id> [--once] [--max-cycles N] [--converge-rounds K]
#        [--target-grade G] [--delivery local|pr] [--auto-approve] [-w <workspace>]
set -euo pipefail

# ── Arg parsing ─────────────────────────────────────────────────────────────
PROJECT_ID=""
WORKSPACE="${NEWTON_WORKSPACE:-$(pwd)}"
ONCE=false
MAX_CYCLES=""
CONVERGE_ROUNDS=""
TARGET_GRADE=""
DELIVERY=""
AUTO_APPROVE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --once)            ONCE=true; shift ;;
    --max-cycles)      MAX_CYCLES="$2"; shift 2 ;;
    --converge-rounds) CONVERGE_ROUNDS="$2"; shift 2 ;;
    --target-grade)    TARGET_GRADE="$2"; shift 2 ;;
    --delivery)        DELIVERY="$2"; shift 2 ;;
    --auto-approve)    AUTO_APPROVE=true; shift ;;
    -w|--workspace)    WORKSPACE="$2"; shift 2 ;;
    -*)                echo "Unknown option: $1" >&2; exit 1 ;;
    *)
      if [ -z "$PROJECT_ID" ]; then PROJECT_ID="$1"; else echo "Unexpected arg: $1" >&2; exit 1; fi
      shift ;;
  esac
done

if [ -z "$PROJECT_ID" ]; then
  echo "Usage: optimize.sh <project_id> [options]" >&2
  exit 1
fi

CONF="${WORKSPACE}/.newton/configs/${PROJECT_ID}.conf"
if [ ! -f "$CONF" ]; then
  echo "Config not found: $CONF" >&2
  exit 1
fi

# ── Load config ──────────────────────────────────────────────────────────────
# shellcheck source=/dev/null
source "$CONF"

REPO_ID="${optimize_repo_id:-}"
REPO_PATH="${optimize_repo_path:-$(pwd)}"
TEST_CMD="${optimize_test_cmd:-./scripts/run-tests.sh}"
GRADERS="${optimize_graders:-}"
: "${MAX_CYCLES:=${optimize_max_cycles:-8}}"
: "${CONVERGE_ROUNDS:=${optimize_converge_rounds:-2}}"
: "${TARGET_GRADE:=${optimize_target_grade:-}}"
: "${DELIVERY:=${delivery:-local}}"
: "${AUTO_APPROVE:=${optimize_auto_approve:-false}}"
MAX_FAILED_ATTEMPTS="${optimize_max_failed_attempts:-2}"
REGRESSION_TOLERANCE="${optimize_regression_tolerance:-3}"
DEVELOP_ENGINE="${optimize_develop_engine:-${develop_primary_engine:-codex}}"
DEVELOP_MODEL="${optimize_develop_model:-${develop_primary_model:-}}"

# Operate from the workspace so `newton data` / `newton workflow run` resolve it
# via cwd discovery (most data calls below do not pass --workspace explicitly).
cd "${WORKSPACE}" || { echo "workspace not found: ${WORKSPACE}" >&2; exit 1; }

GRADERS_LIST=()
read -ra GRADERS_LIST <<< "$GRADERS"

if [ -z "$REPO_ID" ]; then
  echo "optimize_repo_id must be set in $CONF" >&2
  exit 1
fi

# ── Trajectory file ──────────────────────────────────────────────────────────
TRAJ_DIR="${WORKSPACE}/.newton/optimize/${PROJECT_ID}"
mkdir -p "$TRAJ_DIR"
TRAJ="${TRAJ_DIR}/trajectory.jsonl"


log() { echo "[optimize] $*" >&2; }
traj_append() { echo "$1" >> "$TRAJ"; }

# ── Deterministic-grader K=1 forcing (§13.7) ──────────────────────────────────
# If ALL active graders are marked deterministic in the conf, force CONVERGE_ROUNDS=1.
# A deterministic grader's second identical round proves nothing.
_all_deterministic=true
for _g in "${GRADERS_LIST[@]:-}"; do
  _det_var="optimize_grader_deterministic_${_g}"
  if [ "${!_det_var:-false}" != "true" ]; then
    _all_deterministic=false
    break
  fi
done
if [ "$_all_deterministic" = "true" ] && [ "${#GRADERS_LIST[@]}" -gt 0 ]; then
  CONVERGE_ROUNDS=1
  log "All graders are deterministic — forcing CONVERGE_ROUNDS=1 (§13.7)"
fi

# ── Helper: read trajectory into arrays ──────────────────────────────────────
traj_grades() {
  # Emit the last N grade values for a given grader dimension key
  local grader="$1" n="${2:-9999}"
  if [ ! -f "$TRAJ" ]; then echo; return; fi
  grep '"grader":"'"$grader"'"' "$TRAJ" | tail -n "$n" \
    | python3 -c "
import sys, json
for line in sys.stdin:
    try:
        d = json.loads(line)
        print(d.get('grade', ''))
    except: pass"
}

traj_last_decision() {
  if [ ! -f "$TRAJ" ]; then echo ""; return; fi
  tail -1 "$TRAJ" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('decision',''))" 2>/dev/null || echo ""
}

traj_consecutive_none() {
  # Count trailing consecutive decision=none rounds
  if [ ! -f "$TRAJ" ]; then echo 0; return; fi
  python3 - "$TRAJ" <<'PY'
import sys, json
rows = []
with open(sys.argv[1]) as f:
    for line in f:
        line = line.strip()
        if not line: continue
        try: rows.append(json.loads(line))
        except: pass
count = 0
for r in reversed(rows):
    if r.get('decision') == 'none':
        count += 1
    else:
        break
print(count)
PY
}

traj_blocked_count() {
  # Count Findings with status=blocked for this repo
  newton data get findings 2>/dev/null \
    | REPO_ID="$REPO_ID" python3 -c "
import sys, json, os
repo_id = os.environ.get('REPO_ID','')
data = json.load(sys.stdin)
blocked = [f for f in data if f.get('status') == 'blocked' and (not repo_id or f.get('repoId') == repo_id)]
print(len(blocked))" 2>/dev/null || echo 0
}

traj_prev_grade() {
  local grader="$1"
  if [ ! -f "$TRAJ" ]; then echo ""; return; fi
  grep '"grader":"'"$grader"'"' "$TRAJ" | tail -2 | head -1 \
    | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('grade',''))" 2>/dev/null || echo ""
}

traj_no_progress_count() {
  # Count cycles where grade AND open_findings unchanged for a grader
  local grader="$1"
  if [ ! -f "$TRAJ" ]; then echo 0; return; fi
  python3 - "$TRAJ" "$grader" <<'PY'
import sys, json
path, grader = sys.argv[1], sys.argv[2]
rows = []
with open(path) as f:
    for line in f:
        line = line.strip()
        if not line: continue
        try: d = json.loads(line)
        except: continue
        if d.get('grader') == grader:
            rows.append(d)
count = 0
for r in reversed(rows):
    if rows.index(r) == 0:
        break
    prev = rows[rows.index(r) - 1]
    if r.get('grade') == prev.get('grade') and r.get('open_findings') == prev.get('open_findings'):
        count += 1
    else:
        break
print(count)
PY
}

# ── Single grader cycle: grade → reconcile → change-request ──────────────────
run_grading() {
  local grader="$1" scope="repo" scope_id="$REPO_ID"
  local grader_script="${WORKSPACE}/.newton/grader/${grader}/generate.sh"
  if [ ! -x "$grader_script" ]; then
    log "WARNING: grader script not found or not executable: $grader_script"
    echo "none"
    return
  fi
  local grader_cmd="${grader_script} ${scope_id} ${REPO_PATH}"
  # grade → reconcile → change-request. Workflow output is logged to stderr; the
  # decision + CR id are read from the store, because the terminal echo
  # ("PROPOSED <id>") uses capture_stdout:false and does not reliably reach the
  # CLI's stdout.
  newton workflow run "${WORKSPACE}/.newton/workflows/grading.yaml" \
    --trigger "grader=${grader}" \
    --trigger "grader_cmd=${grader_cmd}" \
    --trigger "scope=${scope}" \
    --trigger "scope_id=${scope_id}" >&2 || true
  local cr_id
  cr_id=$(newton data get change-requests 2>/dev/null \
    | RID="$scope_id" python3 -c "
import sys, json, os
data = json.load(sys.stdin)
rid = os.environ.get('RID','')
props = sorted((c for c in data if c.get('repoId') == rid and c.get('status') == 'proposed'),
               key=lambda c: c.get('createdAt',''))
print(props[-1]['id'] if props else '')" 2>/dev/null || echo "")
  if [ -n "$cr_id" ]; then echo "PROPOSED ${cr_id}"; else echo "CONVERGED"; fi
}

# ── Approve CR ───────────────────────────────────────────────────────────────
approve_cr() {
  local cr_id="$1"
  if [ "$AUTO_APPROVE" = "true" ] || [ "$AUTO_APPROVE" = "1" ]; then
    newton data patch change-request "$cr_id" --body '{"status":"approved"}' > /dev/null
    log "Auto-approved CR $cr_id"
  else
    log "HIL gate: approve Change Request $cr_id before continuing"
    log "(Run: newton data patch change-request $cr_id --body '{\"status\":\"approved\"}')"
    # Block on ailoop HIL gate
    ailoop --workspace "$WORKSPACE" --channel "optimize-${PROJECT_ID}" \
      --message "Approve Change Request $cr_id? (yes/no)" \
      --expect yes || { log "CR rejected by human; halting."; return 1; }
    newton data patch change-request "$cr_id" --body '{"status":"approved"}' > /dev/null
  fi
}

# ── Run planner ──────────────────────────────────────────────────────────────
run_planner() {
  local cr_id="$1"
  newton workflow run "${WORKSPACE}/.newton/workflows/planner.yaml" \
    --trigger "change_request_id=${cr_id}" \
    --trigger "workspace=${WORKSPACE}" \
    --trigger "develop_primary_engine=${DEVELOP_ENGINE}" \
    --trigger "develop_primary_model=${DEVELOP_MODEL}" >&2 || true
  # Read the Plan the planner wrote for this CR from the store.
  newton data get plans 2>/dev/null \
    | CR="$cr_id" python3 -c "
import sys, json, os
data = json.load(sys.stdin)
cr = os.environ.get('CR','')
plans = sorted((p for p in data if p.get('linkedChangeRequestId') == cr),
               key=lambda p: p.get('createdAt',''))
print(plans[-1]['id'] if plans else '')" 2>/dev/null || echo ""
}

# ── Run develop ──────────────────────────────────────────────────────────────
run_develop() {
  local plan_id="$1"
  newton workflow run "${WORKSPACE}/.newton/workflows/develop.yaml" \
    --trigger "plan_id=${plan_id}" \
    --trigger "workspace=${WORKSPACE}" \
    --trigger "delivery=${DELIVERY}" \
    --trigger "test_cmd=${TEST_CMD}"
}

# ── Break condition helpers ──────────────────────────────────────────────────
check_regression() {
  local grader="$1" current_grade="$2"
  local prev
  prev=$(traj_prev_grade "$grader")
  if [ -z "$prev" ]; then return 0; fi
  local tol
  local tol_var="optimize_regression_tolerance_${grader}"
  tol="${!tol_var:-$REGRESSION_TOLERANCE}"
  python3 -c "
import sys
prev, curr, tol = float('$prev'), float('$current_grade'), float('$tol')
sys.exit(0 if (prev - curr) <= tol else 1)
" && return 0 || return 1
}

check_target_grade() {
  local grader="$1" current_grade="$2"
  local tgt_var="optimize_target_grade_${grader}"
  local tgt="${!tgt_var:-$TARGET_GRADE}"
  if [ -z "$tgt" ]; then return 1; fi  # no target = not met (skip check)
  python3 -c "import sys; sys.exit(0 if float('$current_grade') >= float('$tgt') else 1)"
}

# ── UUID helper ─────────────────────────────────────────────────────────────
new_uuid() { python3 -c "import uuid; print(uuid.uuid4())"; }

# ── Trap: mark run as failed on unexpected exit ───────────────────────────────
RUN_ID=""
_on_exit() {
  local code=$?
  if [ -n "$RUN_ID" ] && [ "$code" -ne 0 ]; then
    newton data patch optimize-run "$RUN_ID" --body '{"status":"failed"}' > /dev/null 2>&1 || true
  fi
}
trap _on_exit EXIT

# ── Main loop ────────────────────────────────────────────────────────────────
CYCLE=0
log "Starting optimize loop for project=${PROJECT_ID} repo=${REPO_ID} graders=${GRADERS}"
log "max_cycles=${MAX_CYCLES} converge_rounds=${CONVERGE_ROUNDS} delivery=${DELIVERY} auto_approve=${AUTO_APPROVE}"

# ── Create OptimizeRun record ────────────────────────────────────────────────
GRADERS_JSON=$(python3 -c "import json,sys; print(json.dumps(sys.argv[1:]))" "${GRADERS_LIST[@]}")
RUN_ID=$(new_uuid)
newton data post optimize-run --body "$(python3 -c "import json,sys; print(json.dumps({'id':sys.argv[1],'projectId':sys.argv[2],'scope':'repo','scopeId':sys.argv[3],'maxCycles':int(sys.argv[4]),'graders':json.loads(sys.argv[5])}))" "$RUN_ID" "$PROJECT_ID" "$REPO_ID" "$MAX_CYCLES" "$GRADERS_JSON")" > /dev/null 2>&1 || true
log "OptimizeRun created: $RUN_ID"

while true; do
  CYCLE=$((CYCLE + 1))
  log "=== Cycle ${CYCLE}/${MAX_CYCLES} ==="

  # Break: max cycles
  if [ "$CYCLE" -gt "$MAX_CYCLES" ]; then
    log "STOP: max_cycles (${MAX_CYCLES}) reached"
    traj_append '{"cycle":'$CYCLE',"event":"stop","reason":"max_cycles"}'
    break
  fi

  CR_ID=""
  DECISION="none"
  GRADER_GRADES=()

  for grader in "${GRADERS_LIST[@]}"; do
    log "Grading with: $grader"
    GRADING_OUT=$(run_grading "$grader" || true)

    # Extract grade from most recent EvalRun
    CURRENT_GRADE=$(newton data get eval-runs --scope repo --scope-id "$REPO_ID" 2>/dev/null \
      | GRADER="$grader" python3 -c "
import sys, json, os
data = json.load(sys.stdin)
grader = os.environ.get('GRADER','')
# find most recent score for this grader
for r in reversed(data):
    if r.get('source','').startswith(grader):
        score = r.get('score')
        if score is not None:
            print(score)
            sys.exit(0)
print(0)" 2>/dev/null || echo "0")

    OPEN_FINDINGS=$(newton data get findings 2>/dev/null \
      | REPO_ID="$REPO_ID" python3 -c "
import sys, json, os
data = json.load(sys.stdin)
repo_id = os.environ.get('REPO_ID','')
open_s = {'awaiting_triage','triaged','approved_for_planning'}
open_f = [f for f in data if f.get('repoId') == repo_id and f.get('status') in open_s]
print(len(open_f))" 2>/dev/null || echo "0")

    # Break: regression guard (per-grader disjunction)
    if ! check_regression "$grader" "$CURRENT_GRADE"; then
      log "STOP: regression detected for grader=${grader} (grade dropped beyond tolerance)"
      traj_append '{"cycle":'$CYCLE',"grader":"'"$grader"'","grade":'$CURRENT_GRADE',"event":"stop","reason":"regression"}'
      exit 1
    fi

    # Break: no-progress guard (per-grader)
    NO_PROG=$(traj_no_progress_count "$grader")
    if [ "$NO_PROG" -ge "$CONVERGE_ROUNDS" ] && [ "$CYCLE" -gt "$CONVERGE_ROUNDS" ]; then
      log "STOP: no-progress for grader=${grader} for ${NO_PROG} consecutive cycles"
      traj_append '{"cycle":'$CYCLE',"grader":"'"$grader"'","grade":'$CURRENT_GRADE',"event":"stop","reason":"no_progress"}'
      exit 1
    fi

    # Break: grade target (per-grader conjunction — check all at end)
    GRADER_GRADES+=("$grader:$CURRENT_GRADE")

    # Extract decision and CR ID from grading output
    if echo "$GRADING_OUT" | grep -q "^PROPOSED "; then
      DECISION="propose"
      CR_ID=$(echo "$GRADING_OUT" | grep "^PROPOSED " | awk '{print $2}')
    fi

    traj_append '{"cycle":'$CYCLE',"grader":"'"$grader"'","grade":'$CURRENT_GRADE',"open_findings":'$OPEN_FINDINGS',"decision":"'"$DECISION"'","timestamp":"'"$(date -u +%Y-%m-%dT%H:%M:%SZ)"'"}'
    log "grader=${grader} grade=${CURRENT_GRADE} open_findings=${OPEN_FINDINGS} decision=${DECISION}"
  done

  # ── Mirror cycle to store ─────────────────────────────────────────────────
  BLOCKED_COUNT_NOW=$(traj_blocked_count)
  GRADES_OBJ=$(python3 -c "
import json, sys
grades = {}
for entry in sys.argv[1:]:
    grader, grade = entry.rsplit(':', 1)
    try: grades[grader] = float(grade)
    except: grades[grader] = 0
print(json.dumps(grades))" "${GRADER_GRADES[@]-}")
  GRADE_MIN_VAL=$(python3 -c "
import json, sys
d = json.loads(sys.argv[1])
vals = list(d.values())
print(min(vals) if vals else 0)" "$GRADES_OBJ")
  CYCLE_ID=$(new_uuid)
  newton data post optimize-cycle --body "$(python3 -c "
import json,sys
print(json.dumps({'id':sys.argv[1],'runId':sys.argv[2],'cycle':int(sys.argv[3]),'grades':json.loads(sys.argv[4]),'gradeMin':float(sys.argv[5]),'decision':sys.argv[6],'openFindings':int(sys.argv[7]),'resolvedThisCycle':0}))" "$CYCLE_ID" "$RUN_ID" "$CYCLE" "$GRADES_OBJ" "$GRADE_MIN_VAL" "$DECISION" "$OPEN_FINDINGS")" > /dev/null 2>&1 || true
  newton data patch optimize-run "$RUN_ID" --body "$(python3 -c "
import json,sys
print(json.dumps({'cycle':int(sys.argv[1]),'latestGrades':json.loads(sys.argv[2]),'openFindings':int(sys.argv[3]),'blockedFindings':int(sys.argv[4])}))" "$CYCLE" "$GRADES_OBJ" "$OPEN_FINDINGS" "$BLOCKED_COUNT_NOW")" > /dev/null 2>&1 || true

  # Break: grade target — conjunction (all graders clear their threshold)
  if [ -n "$TARGET_GRADE" ]; then
    ALL_TARGETS_MET=true
    for entry in "${GRADER_GRADES[@]}"; do
      grader="${entry%%:*}"
      grade="${entry##*:}"
      if ! check_target_grade "$grader" "$grade"; then
        ALL_TARGETS_MET=false
        break
      fi
    done
    if [ "$ALL_TARGETS_MET" = "true" ]; then
      log "STOP: all grade targets met"
      traj_append '{"cycle":'$CYCLE',"event":"stop","reason":"target_grade"}'
      break
    fi
  fi

  # Break: converged (no actionable work AND no blocked Findings)
  CONSECUTIVE_NONE=$(traj_consecutive_none)
  BLOCKED_COUNT=$(traj_blocked_count)
  if [ "$DECISION" = "none" ]; then
    if [ "$BLOCKED_COUNT" -gt 0 ]; then
      # No actionable work but blocked Findings remain → stalled_on_blocked
      if [ "$CONSECUTIVE_NONE" -ge 1 ]; then
        log "STOP: stalled_on_blocked — ${BLOCKED_COUNT} blocked Finding(s) remain, no actionable work"
        traj_append '{"cycle":'$CYCLE',"event":"stop","reason":"stalled_on_blocked","blocked_findings":'$BLOCKED_COUNT'}'
        break
      fi
    else
      if [ "$CONSECUTIVE_NONE" -ge "$CONVERGE_ROUNDS" ]; then
        log "STOP: converged — decision=none for ${CONSECUTIVE_NONE} consecutive rounds, zero blocked Findings"
        traj_append '{"cycle":'$CYCLE',"event":"stop","reason":"converged"}'
        break
      fi
    fi
    log "No actionable findings this cycle (${CONSECUTIVE_NONE}/${CONVERGE_ROUNDS} none-rounds)"
    if [ "$ONCE" = "true" ]; then break; fi
    continue
  fi

  if [ -z "$CR_ID" ]; then
    log "WARNING: decision=propose but no CR ID captured; skipping cycle"
    if [ "$ONCE" = "true" ]; then break; fi
    continue
  fi

  log "Change Request: $CR_ID"

  # Step 3: Approve
  if ! approve_cr "$CR_ID"; then
    log "HALT: CR approval failed"
    exit 1
  fi

  # Step 4: Plan
  log "Running planner for CR $CR_ID"
  PLAN_ID=$(run_planner "$CR_ID")
  if [ -z "$PLAN_ID" ]; then
    log "WARNING: planner did not return a plan_id; skipping cycle"
    if [ "$ONCE" = "true" ]; then break; fi
    continue
  fi
  log "Plan created: $PLAN_ID"
  traj_append '{"cycle":'$CYCLE',"event":"plan","plan_id":"'"$PLAN_ID"'","change_request_id":"'"$CR_ID"'"}'

  # Step 5: Develop (with failed-plan quarantine logic)
  DEVELOP_STATUS="success"
  if ! run_develop "$PLAN_ID"; then
    DEVELOP_STATUS="failed"
    log "Develop failed for plan $PLAN_ID"

    # Increment attempts on Plan
    CURRENT_ATTEMPTS=$(newton data get plan "$PLAN_ID" 2>/dev/null \
      | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('attempts',0))" 2>/dev/null || echo 0)
    NEW_ATTEMPTS=$((CURRENT_ATTEMPTS + 1))
    newton data patch plan "$PLAN_ID" --body '{"status":"failed","attempts":'"$NEW_ATTEMPTS"',"lastError":"develop failed"}' > /dev/null || true

    if [ "$NEW_ATTEMPTS" -ge "$MAX_FAILED_ATTEMPTS" ]; then
      log "Plan $PLAN_ID reached max failed attempts (${MAX_FAILED_ATTEMPTS}); marking linked Findings as blocked"
      # Get finding IDs from CR
      FINDING_IDS=$(newton data get change-request "$CR_ID" 2>/dev/null \
        | python3 -c "import sys,json; d=json.load(sys.stdin); print(' '.join(d.get('findingIds',[])))" 2>/dev/null || echo "")
      for fid in $FINDING_IDS; do
        newton data patch finding "$fid" --body '{"status":"blocked"}' > /dev/null || true
        log "Finding $fid → blocked"
      done
      traj_append '{"cycle":'$CYCLE',"event":"quarantine","plan_id":"'"$PLAN_ID"'","change_request_id":"'"$CR_ID"'","blocked_findings":"'"$FINDING_IDS"'"}'
    fi
  else
    newton data patch plan "$PLAN_ID" --body '{"status":"complete"}' > /dev/null || true
    log "Plan $PLAN_ID → complete"
  fi

  traj_append '{"cycle":'$CYCLE',"event":"develop","plan_id":"'"$PLAN_ID"'","status":"'"$DEVELOP_STATUS"'"}'

  if [ "$ONCE" = "true" ]; then
    log "--once: stopping after one cycle"
    break
  fi
done

log "Optimize loop finished. Trajectory: $TRAJ"
# Map the last recorded stop reason to a canonical OptimizeRun status
# (running|converged|stalled_on_blocked|max_cycles|regressed|no_progress) instead
# of an out-of-vocabulary "complete" that downstream consumers (UI) can't render.
STOP_REASON=$(grep '"event":"stop"' "$TRAJ" 2>/dev/null | tail -1 \
  | python3 -c "import sys,json; print(json.loads(sys.stdin.readline() or '{}').get('reason',''))" 2>/dev/null || echo "")
case "$STOP_REASON" in
  converged|target_grade) FINAL_STATUS="converged" ;;
  stalled_on_blocked)     FINAL_STATUS="stalled_on_blocked" ;;
  regression|regressed)   FINAL_STATUS="regressed" ;;
  no_progress)            FINAL_STATUS="no_progress" ;;
  *)                      FINAL_STATUS="max_cycles" ;;
esac
log "Final status: ${FINAL_STATUS} (stop reason: ${STOP_REASON:-none})"
newton data patch optimize-run "$RUN_ID" --body "{\"status\":\"${FINAL_STATUS}\"}" > /dev/null 2>&1 || true
