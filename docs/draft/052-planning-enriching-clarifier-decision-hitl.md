# SPEC-052 — Planning enrich workflow: clarifier + structured HITL

**Tracking:** [gonewton/newton#310](https://github.com/gonewton/newton/issues/310)

## Status

Draft. Typical edit location: consumer workspace `.newton/workflows/planning_enriching.yaml`. Newton product templates may ship a copy under `templates/` or docs; keep this spec aligned with the graph authors actually run.

## Problem

After enrichment, `check_gaps` detects `NEED_USER_INPUT` but the human step only offers **Continue / Skip** and asks the user to manually edit the spec. There is no automated clarifier pass and no presentation of grounded **alternatives** and **recommendation** through ailoop’s structured `Decision` UI.

## Goals

1. Insert an **AgentOperator** step after `has_gaps` that runs instructions equivalent to the Cursor command **newton-clarify-question** (same content as `.cursor/commands/newton-clarify-question.md` in a workspace that installs it): read `triggers.output_path`, extract gaps, ground alternatives in the repo, write **valid clarify JSON** (schema from that command) to e.g. `.newton/plan/clarify.json`.
2. Replace or augment **`HumanDecisionOperator`** so the human sees an ailoop **Decision** built from that JSON (mapped fields: problem/context → `context_markdown`, alternatives → `options` with `id`/`label`/pros-cons as `detail_markdown`, recommendation → `recommendation`).
3. Branch transitions on **selected option `id`** (not only Continue/Skip), e.g. `use_recommendation` vs specific alternative ids, plus an explicit **abort / defer** option if desired.
4. Update **`merge_spec`** prompt to consume `clarify.json` + chosen option id and resolve placeholders in the spec accordingly.

## Non-goals

- Changing the enrich agent’s gap taxonomy in `gaps.txt` (unless required for clarify output).
- Implementing Newton `HumanDecisionOperator` itself (see REQ-050 in this repo under `docs/draft/`); this spec assumes that capability exists or documents a temporary `CommandOperator` + `ailoop ask --payload` bridge.

## Mapping (informative)

| Clarify JSON | Ailoop `Decision` field |
|--------------|-------------------------|
| `problem_statement` + `need_user_input` | `context_markdown` (and short `summary`) |
| `alternatives[].id` / `title` / pros+cons | `options[].id` / `label` / `detail_markdown` |
| `recommendation.primary_alternative_id` + text | `recommendation.option_id` + `rationale_markdown` |
| stable run key | `decision_id` |

## Acceptance criteria

1. Running the workflow with a spec that still contains `NEED_USER_INPUT` produces `clarify.json` before HITL.
2. Human receives a structured decision in ailoop (server or direct mode) with visible options and recommendation when present.
3. `merge_spec` (or follow-up agent) applies the chosen resolution; final spec has gaps resolved or explicitly documented per chosen path.
4. Document in `.newton/README.md` or Newton `skills/references` how to run `planning_enriching` with the new graph.

## Dependencies

- REQ-050: Newton `HumanDecisionOperator` + interviewer using ailoop `ask_decision`.
- Optional: validated JSON schema step (`CommandOperator` + `jq`/small validator) before HITL.

## File to edit

- From the workspace `.newton` root: `workflows/planning_enriching.yaml`
