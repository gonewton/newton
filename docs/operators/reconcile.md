# ReconcileOperator

> Status: **design** (not yet implemented). See [CONTEXT.md](../../CONTEXT.md)
> (Reconciliation, Observation, Finding) and
> [ADR 0009](../adr/0009-grading-is-text-gradient-driven.md).

Reconciles this run's transient **Observations** into the durable **Finding**
set: it gives Findings stable identity across non-deterministic grading runs and
tracks resolution. Runs as an in-graph step after grading (not a sink — it is
active read-match-write, may call an LLM, can fail, and feeds the loop).

## What it does

1. **Reads** the **Findings** for the scope from the backend store — **all
   statuses, not just open** (it must see `rejected`/`deferred`/`resolved` ones to
   avoid resurrecting won't-fix items and to reopen fixed ones) — plus this run's
   new **Observations** (from the grade task output).
2. **Prefilters deterministically** — natural-key fingerprint
   (`scope + dimension + normalized location + rule`) resolves the obvious
   matches without an LLM.
3. **Adjudicates the ambiguous remainder** with a structured agent (see
   "Structured I/O" below): input `{ unmatched_observations, candidate_findings }`
   → output a **reconciliation plan** `{ matched, new, resolved }`. The agent
   *judges sameness only* — low temperature; it does not touch the store.
4. **Applies the plan deterministically** (this is the operator's job, not the
   LLM's). The status transitions respect human decisions:

   | Situation | Action |
   | --- | --- |
   | matches an `open/active` Finding | **refresh** `last_seen_at`; status unchanged |
   | matches a `resolved` Finding | **reopen** → `awaiting_triage` (it came back) |
   | matches a `rejected`/`deferred` Finding | refresh `last_seen_at`, **keep status** — never resurface/recreate (respect won't-fix) |
   | matches nothing | **create** a Finding (`awaiting_triage`) |
   | `open/active` Finding **not seen** this run | **resolve** (auto) |
   | `rejected`/`deferred`/`resolved` Finding not seen | no change |

5. **Returns** counts `{ created, refreshed, reopened, resolved }` for the loop.

`resolved` is auto ("it got fixed") and firmly distinct from human
`rejected`/`deferred` ("won't / not now"). Only open/active Findings auto-resolve;
a re-reported `rejected` Finding is refreshed-but-suppressed, never recreated —
this is what stops the loop churning on won't-fix items every cycle. The auto
`resolve` is the per-Finding convergence signal ("present last run, gone now →
the gradient landed"), richer than the scalar Grade delta.

## Why hybrid (not pure-LLM)

We use a stochastic tool to enforce *stable* identity, so variance must be
bounded: the deterministic prefilter handles the easy cases, the LLM only the
genuinely-ambiguous ones, and once a match lands the Finding has a **stable
Newton id** that subsequent runs anchor to (matching against a fixed id, not
drifting text). Embeddings may shortlist candidates before the LLM as a scale
optimization.

## Structured I/O (implementation constraint)

The adjudication step **must use aikit-sdk structured input/output** — the
`Pipeline` (typed schema-in → schema-out → validate → retry), the same path dk
and the other smart operators use. Not the signal-based `AgentOperator`, not
custom agent plumbing. This applies to all three smart operators
(`GraderAgentOperator`, `ReconcileOperator`, `ChangeRequestOperator`): they share
one structured-agent core, differing only in input source and output schema.

## Holds

Backend-store handle (read open Findings; write create/refresh/resolve) + the
structured-agent engine — both constructor-injected, per the `AgentOperator`
precedent.
