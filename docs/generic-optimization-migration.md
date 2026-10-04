# Generic optimization migration

Optimization Definition schema version 2 replaces the software-specific native
workflow contract. This is a breaking change for optimization definitions,
workflow result maps, saved runs, and consumers that assumed SQLite projection.
Ordinary workflow execution and the optional catalog APIs are unchanged.

## Definition changes

- Set `schema_version: 2`.
- Replace the old strategy with either
  `strategy: {kind: observation_driven, max_suggestions: 5}` or
  `strategy: {kind: measurement_driven}`.
- Rename native roles to required `propose` and optional `execute`. The evaluator
  remains selected through `requirements.objective` and `requirements.evaluators`.
- Add `requirements.stagnation_cycles` or accept the default of three.
- Remove `workflows.promote`; delivery happens after optimization through a
  domain-specific adapter or workflow.

## Workflow output changes

Evaluator workflows return `EvaluationOutput` (`candidate`, `evaluation`, and an
optional `assessment`). Proposal workflows return tagged `ProposalOutput` and
may return a candidate directly. Optional execution returns `ExecutionOutput`.
The old `GradeOutput`, `PlanOutput`, and `DevelopOutput` envelopes and their
required Change Request/Finding/Plan correlation are removed from the native
optimizer.

Observation-driven definitions select assessment-local observations. Existing
global Findings can remain in catalog workflows, but the generic optimizer does
not reconcile or mutate them. Measurement-driven definitions should omit
observations and Plans when they add no value.

## Persistence and integrations

New runs use `run.json`, `current.json`, immutable `cycles/*.json`,
`outcome.json`, and `report.json`. They do not write `backend.sqlite` and do not
automatically appear in the legacy SQLite-backed optimize-run HTTP endpoints or
UI. File observers should use `OptimizeRunObservationSource` or read the
versioned records after an `optimize_run_update` invalidation event.

Pre-version-2 runs containing only `journal.json` and SQLite rows cannot resume
with the new driver. Finish them with Newton 0.5.133 or retain them as historical
evidence and start a new run. There is no implicit migration because workflow
outputs and candidate semantics changed.

Requirements revisions preserve retained artifacts separately from current
qualification. Consumers should show both `retained_result` and
`accepted_result`; absence of the latter after a revision is not loss of the
former.

## Validation sequence

```bash
newton optimize <project> --definition definition.yaml --inspect
newton optimize <project> --definition definition.yaml --preflight
newton optimize <project> --definition definition.yaml --once
```

Check `.newton/state/optimize/<run-id>/report.json` and validate the candidate,
measurements, constraints, stop reason, and domain-specific delivery result.
