# ChangeRequestOperator

> Status: **design** (not yet implemented). See [CONTEXT.md](../../CONTEXT.md)
> (Change Request, Finding, Reconciliation) and
> [ADR 0009](../adr/0009-grading-is-text-gradient-driven.md).

The loop's **optimizer step**: reads the standing **Findings** (the aggregated
text-gradient), synthesizes a coherent **Change Request**, and persists it. It is
a *structured-agent* operator — the same machinery as the rubric
`GraderAgentOperator` and dk's structured agents (`aikit-sdk::Pipeline`: prompt +
output schema → agent → validate → retry) — differing only in input source and
output schema:

- rubric Grader: `Rubric + state → Assessment`
- this operator: `Findings → Change Request | none`

## What it does

1. **Reads** the open **Findings** for the scope from the backend store (the
   *standing* set across runs/graders — not just the previous task's output),
   plus the latest **Scores**/**Objective**.
2. **Selects/prioritizes** by Score (worst dimension / biggest gap first) — does
   not dump every Finding at the agent (token + focus).
3. **Synthesizes** via a structured agent against an output schema:
   `{ decision: "propose" | "none", change_request?: {…} }`.
4. **Validates** the output against the change-request schema.
5. **Persists** the Change Request (status `proposed`) on `propose`; records a
   no-op on `none`.
6. **Returns** `{ decision, change_request_id? }` to drive the loop.

It is *not* a context-free schema transform: it **reads** the durable Finding set
and **persists** the result, so it holds a backend-store handle
(constructor-injected, like `AgentOperator`'s engine_manager) plus the
structured-agent engine.

## The "none" outcome is first-class

When the prioritized Findings don't warrant a change, the agent returns
`decision: "none"` — the **optimizer step declining**: local convergence /
nothing actionable this cycle (the textual gradient is ~zero). The loop reads
this to **stop (converged)** or **wait for the next grading cycle**. "No Change
Request" is a valid, meaningful result, not an error.

## Params (sketch)

| Param | Notes |
| --- | --- |
| `scope` / `scope_id` | which standing Findings to read. |
| `agent` / `model` | the synthesis engine. |
| `rubric`/`prompt` | synthesis instructions (how to weigh Findings, house style). |
| `output_schema` | the change-request schema (`propose \| none`). |
| `max_findings` / `min_severity` | Score/severity-based selection bounds. |

## Output (`tasks.<id>.output`)

```jsonc
{ "decision": "propose", "change_request_id": "cr_…" }   // or { "decision": "none" }
```

The loop routes on `decision` (propose → plan; none → stop/wait).

## Relation to the pipeline

```
… Findings ──[ChangeRequestOperator]──▶ Change Request (proposed) ──▶ plan ──▶ …
                                       └▶ none ──▶ stop / wait
```

Upstream, **Reconciliation** (Observations → Findings) must have produced the
standing Finding set this operator reads.
