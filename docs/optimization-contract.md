# Generic optimization contract

`newton optimize <project-id>` binds an existing Optimization Definition and
runs one domain-neutral loop:

```text
evaluate incumbent → propose → optionally execute → evaluate candidate → decide
```

Newton owns comparison, stopping, recovery, and history. Definition workflows
own how states are evaluated and changed. A candidate can identify a Git commit,
parameter set, snapshot, object version, or any other immutable local state.
Newton does not create definitions from a goal and does not implicitly publish,
merge, deploy, or apply an accepted candidate.

## Definition version 2

A definition declares one strategy, one or more evaluators, requirements, and
workflow references. The evaluator workflow is referenced by the Objective;
`workflows.propose` is required and `workflows.execute` is optional.

```yaml
schema_version: 2
id: scheduling-parameters
revision: '1'
strategy: {kind: measurement_driven}
workflows:
  propose: propose.yaml
requirements:
  objective:
    mode: primary
    objective:
      id: makespan
      evaluator: simulator
      measurement: {kind: numeric, unit: simulated_minutes, direction: minimize}
  evaluators:
    simulator: {workflow: evaluate.yaml, revision: deterministic-v1}
  comparison: {kind: exact}
  acceptance_constraints:
    - {id: feasible, evaluator: simulator}
  execution_restrictions: {denied_actions: [], protected_paths: []}
  resource_limits:
    {elapsed_seconds: 60, max_cycles: 3, max_work: 3, max_evaluations: 6}
  completion:
    - {kind: objective_target, objective: makespan, target: 4}
  stagnation_cycles: 2
```

`measurement_driven` proposes from measurements or strategy-owned state. It does
not require observations or a Plan. `observation_driven` accepts
`max_suggestions`, which defaults to five. Its proposal may select no more than
that many unique observations, and every selection must identify an observation
from the current assessment. Priority and severity have no universal meaning.

Primary Objectives support native numeric measurements with explicit units and
`minimize` or `maximize`, or rubric Grades from 0 to 100. Threshold mode preserves
independent Grade targets and their regression/no-progress guards. Comparison is
either `exact`, or `repeated` with an exact sample count and minimum improvement.
Acceptance constraints are independent of the Objective. A missing or failed
required check is `unknown` and never passes.

`stagnation_cycles` counts consecutive completed Cycles without an accepted
improvement and resets on acceptance. Resource limits count started cycles, work
dispatches, and evaluator dispatches. Spending and token accounting are outside
this version.

## Evaluator output

Every evaluator invocation returns one `EvaluationOutput`:

```json
{
  "candidate": {
    "id": "candidate-1",
    "artifact_id": "schedule:7",
    "base_artifact_id": "schedule:base",
    "created_under_revision": 1
  },
  "evaluation": {
    "id": "evaluation-1",
    "run_id": "<run-id>",
    "cycle": 1,
    "candidate_id": "candidate-1",
    "artifact_id": "schedule:7",
    "base_artifact_id": "schedule:base",
    "requirements_revision": 1,
    "evaluator_revisions": {"simulator": "deterministic-v1"},
    "measurements": {
      "makespan": {
        "status": "produced",
        "measurement": {
          "kind": "numeric",
          "unit": "simulated_minutes",
          "direction": "minimize"
        },
        "samples": [7.0]
      }
    },
    "constraints": {
      "feasible": {
        "evaluator": "simulator",
        "status": "satisfied",
        "evidence": ["simulator-result.json"]
      }
    },
    "completion_checks": {}
  },
  "assessment": null
}
```

Candidate and evaluation identities, artifact/base references, run, cycle,
requirements revision, evaluator revisions, measurement type, unit, direction,
and required checks must correlate exactly. Operational measurement failure uses
`{"status":"error","message":"..."}`; it is not a low score.

`assessment` is optional. When present it has an assessment-local `id`, summary,
observations, and optional coverage. Observations require `id`, `title`,
`rationale`, and `suggested_action`; priority, evidence, continuity links, and
resolution are optional. A `resolved` claim requires evidence. Complete coverage
also requires evidence. Newton preserves each assessment as emitted and does not
reconcile it into permanent global Findings.

Repeated comparison invokes the evaluator multiple times. History retains every
raw invocation; only the in-memory decision input combines their compatible
samples.

## Proposal and execution outputs

The `propose` workflow returns exactly one tagged `ProposalOutput`:

```json
{"decision":"candidate","proposal_id":"p1","candidate":{...},
 "rationale":"...","selected_observations":[],"plan":null}
```

```json
{"decision":"execute","proposal_id":"p1","rationale":"...",
 "attempt":{...},"selected_observations":[],"plan":{...}}
```

```json
{"decision":"none","reason":"no useful next action"}
```

```json
{"decision":"failed","proposal_id":"p1",
 "failure":{"reason":"...","evidence":["proof that effects are known"]}}
```

A selected observation contains `assessment_id`, `observation_id`, and rationale.
`plan` is optional domain data. A `candidate` proposal skips execution. An
`execute` proposal requires `workflows.execute`, which returns either
`{"status":"candidate","candidate":{...}}` or a known-safe `failed` result.
Safe failures need a nonempty reason and evidence and count toward stagnation.
Uncertain external effects are operational failures and require reconciliation;
Newton will not blindly replay the dispatch.

Each workflow declares `workflow.settings.io.result_map`. Optimization role
workflows must be acyclic and use no unmetered task retries, agent loop mode, or
nested workflow dispatch. Operators receive trigger data including the run and
cycle identities, active requirements, parameters, retained/current accepted
result, earlier cycle attempts, stage, and candidate where applicable.

## Acceptance and stopping

The first candidate that satisfies all constraints is an initial qualification,
not an invented improvement. Later candidates replace the incumbent only when
the declared comparison proves improvement and all constraints pass. Ties,
regressions, inconclusive samples, unknown checks, and violated checks preserve
the retained incumbent.

Newton reports completion independently from why execution stopped. Public stop
reasons are `completed`, `cycle_complete`, `resource_limit`,
`no_actionable_work`, `no_progress`, `regression`, `operational_failure`,
`cancelled`, and `needs_intervention`. No actionable work below the target still
returns the best retained result. A requirements revision can make retained
evidence stale; `retained_result` preserves that history while `accepted_result`
is present only when the result qualifies under the active revision.

## JSON history and recovery

The generic optimizer does not open `backend.sqlite`. Each run is a directory:

```text
.newton/state/optimize/<run-id>/
├── run.json              # immutable binding metadata
├── current.json          # small mutable recovery checkpoint
├── cycles/
│   ├── 0001.json         # immutable completed Cycle
│   └── 0002.json
├── outcome.json          # final typed outcome
└── report.json           # before/after plus all completed Cycles
```

The Cycle file is the commit point. It is created before `current.json` advances.
On resume, a valid published Cycle with an older checkpoint rolls the checkpoint
forward without replaying execution. A conflicting Cycle, malformed identity,
active dispatch, or uncertain effect fails closed. One local claim serializes
writers; it is not a distributed lock.

`run.json`, Cycle records, and reports carry schema versions. Existing runs that
only contain the pre-version-2 `journal.json`/SQLite representation are not
migrated in place. Finish them with Newton 0.5.133 or start a new run.

The core observation source reads these JSON records. `optimize_run_update`
events are invalidation hints; consumers re-read a coherent snapshot. The
SQLite-backed `newton serve` catalog, Findings, Plans, and legacy optimize-run
endpoints remain optional platform features and are not automatically populated
by a generic local run.

## Permissions, delivery, and requirements updates

Definitions may deny actions and protect paths, but ordinary parameters cannot
grant authority. `.newton/configs/<project-id>.conf` supplies the host ceiling:

```text
project_root=.
definition_file=.newton/definitions/scheduling/definition.yaml
optimize_allowed_actions=agent,command,network,commit,draft_pull_request
parameter.model="configured-model"
```

Coding definitions can create an isolated worktree and review branch; a
simulation definition can return a parameter candidate directly. The generic
host rejects `workflows.promote` because it cannot verify and atomically update
an arbitrary target. Applying or publishing the retained result belongs to a
domain adapter or later workflow.

`--requirements-update <file>` is accepted only with `--resume`. Newton preserves
prior evidence and accepted history. A stopped safe run activates an authorized,
valid revision before new work. Unknown in-flight effects prevent activation and
resume until reconciled.

## Operating the loop

```bash
newton optimize demo --definition definition.yaml --inspect
newton optimize demo --definition definition.yaml --preflight
newton optimize demo --definition definition.yaml --once
newton optimize demo --resume <RUN_ID> --once
newton optimize demo --resume <RUN_ID>
```

`--inspect` binds and prints non-secret configuration. `--preflight` compiles the
referenced workflows, validates typed outputs and authority, and dispatches no
agent or candidate. `--once` completes at most one Cycle and records
`cycle_complete` unless another stop guard fires.

The deterministic measurement-only example used by integration tests is under
`crates/cli/tests/fixtures/scheduling/`. The shipped software-security bundle is
a coding-specific example with Pi support. Its detached candidate is not a claim
of comprehensive security or automatic delivery.

The opt-in real-agent gate is:

```bash
python3 scripts/test-optimize-live.py /path/to/workspace default ./target/debug/newton \
  --route configured-provider
```

It requires correlated Pi SDK/tool/terminal evidence and a changed accepted
artifact while the original checkout remains unchanged. Local-gateway
configuration evidence and observed transport evidence are reported separately;
configuration labels alone do not prove routing. Missing configuration is
reported as not exercised. See `docs/testing-generic-optimization.md` for the
deterministic and Docker paths.

## Generated contracts

Rust wire types are authoritative. `newton_core::optimization::schema` exports
JSON Schema values for definitions, candidate evaluations, evaluation outputs,
proposal outputs, execution outputs, Cycle records, requirements updates, and
reports. Runtime validation additionally checks cross-field identities, units,
revisions, authority, K, coverage evidence, and finite samples.

See [ADR 0016](adr/0016-optimization-history-as-json.md), [ADR 0017](adr/0017-domain-neutral-optimization.md), and the [migration guide](generic-optimization-migration.md).
