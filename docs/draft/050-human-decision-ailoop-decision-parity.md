# REQ-050 — HumanDecisionOperator parity with ailoop `Decision` JSON

**Tracking:** [gonewton/newton#311](https://github.com/gonewton/newton/issues/311)

## Status

Draft requirement. Tracks GitHub issue on `gonewton/newton` (see issue body for number).

## Problem

Newton `HumanDecisionOperator` and the ailoop interviewer ([`crates/core/src/workflow/human/ailoop.rs`](../../crates/core/src/workflow/human/ailoop.rs)) still use legacy `prompt` plus flat string `choices` and the removed / non-Decision ailoop client path. Ailoop wire format is now **`MessageContent::Decision`** with structured fields aligned to JSON:

- `decision_id`, `summary`, `context_markdown?`, `options[]` (`id`, `label`, `detail_markdown?`), `recommendation?`, `timeout_seconds`
- Human response: `answer` equals selected **`options[].id`**

Workflow authors cannot express the same payload Newton already needs for clarifier-style HITL without ad-hoc shell bridges.

## Goal

`HumanDecisionOperator` MUST accept parameters that serialize to the same semantic model as ailoop-core `Decision` (field names and validation rules aligned with [`ailoop_core::models::validate_decision`](https://github.com/goailoop/ailoop/blob/main/ailoop-core/src/models/message.rs) and `MessageContent::Decision`).

The Newton ailoop interviewer MUST call **`ailoop_core::client::ask_decision`** (or equivalent) so server, bundled UI, and CLI render one consistent decision card.

## Non-goals

- Changing `HumanApprovalOperator` behavior.
- Defining Newton-side mapping from external schemas (e.g. newton-clarify JSON); that belongs in workflow docs or a separate operator.

## Requirements

### R1 — YAML params

Operator `params` MUST support either:

1. **Structured (preferred):** `decision_id`, `summary`, optional `context_markdown`, `options` (array of objects with `id`, `label`, optional `detail_markdown`), optional `recommendation` (`option_id`, optional `rationale_markdown`), `timeout_seconds` (optional; default from human settings), optional `default_choice` as an **option `id`** (not display label).

2. **Migration:** If the repo still carries old workflows, a single release MAY support deprecated `prompt` + `choices` by compiling them into a synthetic `Decision` (summary = prompt, options derived from labels with auto-generated ids). This spec RECOMMENDS dropping legacy after one release; confirm in implementation PR.

### R2 — Wire / client

- Interviewer trait extended or replaced so implementation builds `MessageContent::Decision` and uses `ask_decision`.
- Answer validation: selected string MUST match one of `options[].id` (and label/index disambiguation follows ailoop server rules).

### R3 — Task output

Task output JSON MUST expose at minimum:

- `choice`: selected option **`id`** (stable for `$expr` transitions).
- `timestamp`, existing timeout/default flags as today.
- Optional: `label` echo from metadata if ailoop returns it (for logs only; transitions SHOULD use `id`).

### R4 — Docs and fixtures

- Update [`docs/operators/human_decision.md`](../operators/human_decision.md) with Decision-shaped YAML and migration note.
- Update workflow graph fixtures / integration tests under `crates/core/tests/fixtures/workflows` for any scenario using `HumanDecisionOperator`.

### R5 — Dependency

Newton `Cargo.toml` MUST depend on an ailoop-core revision that exports `ask_decision`, `DecisionOption`, `DecisionRecommendation`, and `MessageContent::Decision`.

## Acceptance criteria

1. A workflow task can present a decision with `context_markdown` and per-option `detail_markdown` and optional `recommendation`; human answer resolves to the correct option `id` in task output.
2. `default_choice` applies on timeout when set and references a valid option `id`.
3. Invalid params (duplicate ids, recommendation pointing outside options, fewer than two options) fail at workflow validation or operator execute with a clear error code.
4. Documentation and at least one fixture example use the new shape.

## References

- Ailoop: `goailoop/ailoop` `ailoop-core` decision model and `ask_decision`.
- Newton operator: `crates/core/src/workflow/operators/human_decision.rs`.
- Interviewer: `crates/core/src/workflow/human/ailoop.rs`.
