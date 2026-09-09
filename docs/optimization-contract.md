# Optimization Definitions

An Optimization Definition is a reusable YAML or JSON document. It selects a
strategy and workflows, describes the objective and acceptance requirements,
and sets finite resource limits. Loading a definition does not execute code or
grant permissions.

The schema version is `1`. Definition `id` and `revision` identify the reusable
process. Each run retains the complete bound document, context, effective
parameters, and acknowledged Requirements Revision. Editing the source file
does not change an active or resumed run.

```yaml
schema_version: 1
id: verified-security-improvement
revision: "1"
strategy: software-improvement
workflows:
  grade: grading.yaml
  plan: planning.yaml
  develop: development.yaml
requirements:
  objective:
    mode: primary
    objective:
      id: critical_vulnerabilities
      evaluator: security
      measurement:
        kind: numeric
        unit: verified_vulnerabilities
        direction: minimize
  evaluators:
    security:
      workflow: grading.yaml
      revision: protected-evaluator-content-v1
  comparison:
    kind: exact
  acceptance_constraints:
    - id: existing_tests_pass
      evaluator: security
  execution_restrictions:
    denied_actions: [merge, deploy]
    protected_paths: [audit/evaluate.sh]
  resource_limits:
    elapsed_seconds: 3600
    max_cycles: 8
    max_work: 16
    max_evaluations: 32
  completion:
    - kind: objective_target
      objective: critical_vulnerabilities
      target: 0
defaults:
  agent:
    kind: literal
    value: claude
```

The example describes a process; its workflow references and immutable evaluator
revision must be supplied by the execution host. A host that cannot enforce the
declared restrictions rejects the binding rather than claiming protection.
Built-in definitions and user-authored definitions use this same wire contract.

## Shipped dependency-security definition

Install the embedded bundle without a network download:

```sh
newton init . --template builtin
newton optimize default --inspect
```

Normal `init` also installs this bundle after its configured aikit template. For
an existing workspace, reference a previously installed definition through
`--definition /path/to/definition.yaml` or `definition_file`; do not remove the
workspace. Multiple project configs may reference the same unchanged definition.

The bundle supports a committed root `Cargo.toml` and `Cargo.lock`. Its objective
is the number of affected-package/advisory entries reported by `cargo audit`.
It does not cover application flaws, secrets, infrastructure, unsupported package
ecosystems, or compliance. Zero matches is not proof that the product is secure.

Configure `.newton/configs/default.conf` with your existing agent and evaluator:

```ini
definition_file=.newton/definitions/software-security/definition.yaml
parameter.agent=pi
parameter.model=your-configured-model
parameter.advisory_db=/absolute/path/to/rustsec-advisory-db
parameter.advisory_db_revision=full-git-commit-id
parameter.test_command=["cargo","test","--locked"]
```

Prerequisites are Python 3, Cargo, cargo-audit, a prepared RustSec advisory
database at the specified clean Git revision, and an installed/configured Pi,
Claude, or Codex agent. Database fetching and gateway configuration are not
performed by Newton. The scanner uses `--no-fetch` and `--no-yanked`; it validates
JSON output and exit status, and fails on operational errors. See the
[cargo-audit command contract](https://github.com/rustsec/rustsec/blob/main/cargo-audit/src/commands/audit.rs).

The current host runs trusted agent/tool code without a sandbox. Consequently,
its project config must explicitly grant all executable actions:

```ini
optimize_allowed_actions=agent,command,network,commit,draft_pull_request,publish,merge,deploy
```

These broad grants are a limitation, not a recommendation for untrusted code or
agents. The bundle has no promotion workflow and does not itself merge, publish,
or deploy, but it cannot prevent a trusted-host agent from doing so. Candidate
worktrees and after-the-fact checks are not permission or evaluator isolation.

```sh
newton optimize default --preflight
newton optimize default --once
```

Inspection creates no run or candidate and does not test credentials. Preflight
loads/lints workflows, verifies authority and runs bounded prerequisite checks;
it does not dispatch agents, create candidates, or mutate project configuration.
Logging and temporary diagnostic files remain ordinary CLI side effects.
Every new run performs preflight before claiming work or consuming its budget.
Prerequisite execution is currently provided for the shipped security adapter;
custom definitions receive static workflow/authority checks, not an invented
claim that arbitrary evaluator dependencies are ready.

Definitions declare local helper files with `assets: [security.py]`. List every
local helper and evaluator input the workflows require. Paths must remain
relative to the definition directory, without parent traversal or escaping
symlinks. Assets must be UTF-8 and are supplied from host memory through
`triggers.assets["security.py"]`; the persistent snapshot root is not disclosed
to a dispatched role. `WorkflowOperator` is rejected in optimization roles
because its filesystem-loaded child workflows do not yet provide the same
provenance. External executables, libraries and services remain host
prerequisites, not snapshot contents.

After reading the installed source once, Newton compiles every role workflow and
retains declared asset content in process memory, then atomically copies those
same bytes into the run's state directory for durable resume. Dispatch executes
the preloaded document and injects the retained assets; it never reloads a role
or helper from the writable snapshot between checksum checks. The journal pins
SHA-256 hashes and the definition identity. Resume reads and hashes each
persisted file once, then uses those verified bytes for the resumed process.
Editing, transiently replacing, or deleting source files cannot change an
existing process. Persistent snapshot tampering prevents resume. A run without
a snapshot cannot resume. Resume inspection shows the pinned paths and hashes;
new-run inspection shows source hashes without creating state. Preflight executes
the selected installed security helper from retained content, not a substituted
embedded version. This provenance boundary prevents workflow-driven file swaps;
the unsandboxed host is not a security boundary against a process that can alter
Newton's memory or journal.

The adapter audits immutable commits in detached evaluation worktrees. It runs
the configured tests and permits only Cargo manifests/lockfile changes in accepted
candidates. Agent development occurs in a separate detached worktree; snapshot
commits are retained under `refs/newton/candidates/`. The original HEAD and tracked
files are not changed by the adapter. Untracked input files are not evaluated.
Evidence and worktrees remain under `.newton/optimize-artifacts/<RUN_ID>/security/`
for review; cleanup is explicit, not automatic. Inspect the accepted commit before
integrating it through your normal review process.

The bundle uses the `direct-search` strategy and a fixed dependency-remediation
plan, without Findings/Change-Request synthesis. The deterministic CI
tests replace scanner/agent boundaries, execute the distributed graphs against
two repositories, and verify that accepted commits do not replace either HEAD.
They do not prove real-agent quality or actual RustSec coverage.

## Software work and recovery

Both supported strategies execute `grade`, `plan`, and `develop` through the
existing workflow engine. `direct-search` requires no portfolio entities.
`software-improvement` retains the Findings → Change Request → Plan → Execution
model in Newton's store:

- The grade workflow composes the existing grading, reconciliation and CR
  operators as needed, then returns `{candidate, evaluation, change_request_id}`.
- The planner returns `{decision: propose, plan_id, change_request_id}`. Its CR
  must exactly match this cycle's grade output. Newton loads that exact CR and
  ready Plan, verifies their link and eligible Findings, and records both IDs in
  the Cycle. It never picks the latest repository record.
- `{decision: none}` is valid only when grade returned no CR. Missing, malformed
  or failed grading/reconciliation/planning is an operational failure.
- Develop returns `{candidate}` on success. A known-safe failed attempt instead
  returns the explicit envelope below. Missing recovery evidence, crashes and
  malformed output stop for intervention; they are not automatically retried.

```json
{"failure":{"change_request_id":"cr-1","plan_id":"plan-1","reason":"tests failed","reconciled":true,"evidence":["candidate retained; no external action remains in flight"]}}
```

The trusted workflow is responsible for that recovery evidence; Newton does not
infer safe external state from a process exit code. `max_failed_attempts` is a
positive integer parameter, default `2`. The durable per-CR counter spans new
Plans within the Run. A validated candidate rejected by the acceptance policy
also consumes this budget; it does not complete the unresolved CR. Each Plan
counts at most once. An executed Plan is never replayed. Exhaustion marks the
Plan failed and quarantines its linked Findings as `blocked`. Only the existing
authorized Finding-resolution path clears that fence; auto-approval cannot.

Every subsequent grade/planner receives `blocked_work` (CR IDs),
`blocked_work_count`, and the `software_work` ledger. It may explicitly select
unrelated eligible work. Outcomes retain `blocked_work` even if another candidate
reaches its target. No remaining actionable work with blocked CRs stops as
`needs_intervention` (`stalled_on_blocked` in the stored Run). The default bundle
does not supply a software Findings/CR planner; use composed entity-backed
workflows for this strategy.

## Binding and permissions

Ordinary parameters resolve as definition defaults → project settings → explicit
run overrides. The binding is a new snapshot; it does not rewrite unrelated
workspace configuration or require a portfolio hierarchy.

The CLI exposes explicit overrides through repeated `--param NAME=JSON`, for
example `--param 'agent="pi"' --param 'model="configured-model"'`. Values must
be non-secret JSON. `--inspect` shows the flattened effective values; resume
uses its saved binding, so changes require `--requirements-update`.

Parameters are either `{kind: literal, value: ...}` for non-sensitive data or
`{kind: secret_reference, reference: ...}` for a host-resolved credential reference.
Do not put credentials in literals. Preview helpers redact secret references and
never resolve their values.

Execution authority is the intersection of project and environment grants, with
declared denials removed. Parameter overrides cannot add grants. Evaluation
acceptance does not authorize publication, merging, deployment, or rollback.

The host must enforce action denials across agents, commands, and nested
workflows. It must protect authoritative evaluator inputs from candidate writes.
The domain helpers validate declared capabilities; they do not implement a
sandbox. A prompt, a path checksum checked after execution, or a post-hoc score
does not establish an enforceable action prohibition.

## Evaluation and acceptance

Native measurements declare a nonempty unit and `minimize` or `maximize`.
Rubric measurements use `{kind: grade, dimension: ...}` and finite scores from
0 to 100, where larger is better. Independent objectives are not weighted.

Every evaluation identifies its run, cycle, candidate, immutable artifact,
integration base, Requirements Revision, and evaluator/input revisions. Produced
measurements and operational errors are distinct wire variants. Missing or
incompatible samples, changed identities, and evaluator errors fail validation.
Acceptance checks report `satisfied`, `violated`, or `unknown`; only `satisfied`
qualifies. Human-judged checks require an identified reviewer.

Grading executes the workflow referenced by the active requirements' evaluators,
including after an authorized evaluator update; `workflows.grade` cannot override
that selection. Multiple evaluator identities may share one aggregate grading
workflow, executed once per sample. Distinct active evaluator workflows are not
mergeable under `GradeOutput` and fail preflight or update validation explicitly.
Compose their measurements/checks in one workflow instead. Alternate evaluator
workflows must already be declared and preloaded in the immutable definition
snapshot, for example as an additional workflow reference.

The first result passing evaluation and acceptance checks is an initial
qualification, not an improvement over a nonexistent accepted result. Initially
failing checks do not prevent starting a run. Subsequent candidates must improve
under the declared comparison policy. Exact ties preserve the accepted result.

For noisy primary measurements, use:

```yaml
comparison:
  kind: repeated
  samples: 3
  min_improvement: 2.0
```

Each evaluation must contain exactly three samples. The worst candidate sample
must improve on the best accepted sample by at least `min_improvement`, measured
in the objective's own units. Overlapping ranges are inconclusive and preserve
the accepted result. This conservative observed-range rule is not a confidence
interval or a statistical significance guarantee.

Threshold mode contains independent Grade objectives, each with `target`,
`regression_delta`, and `no_progress_cycles`. Completion requires all targets.
Incremental acceptance requires at least one improving Grade and no worsening
Grade. The native driver records revision-scoped history and stops before
acceptance when any Grade drops beyond its tolerance from the current Cycle's
baseline, or a below-target objective exhausts its no-progress limit. No aggregate
score decides acceptance, stopping or completion.

For `direct-search`, no-progress is score-only. In software threshold mode,
every grade output must also include `open_findings: {objective_id: count, ...}`
for all objectives. These counts belong to the same correlated grade output and
must agree across repeated evaluations. No-progress requires both no Grade
improvement and no reduction in open Findings. A satisfied objective does not
stall improvement of another objective. New Requirements Revisions have separate
history and cannot reuse old-revision evidence.

The host preserves previous accepted artifacts while exploring. The generic
workflow host does not support a `promote` role: it cannot independently inspect
an arbitrary target and atomically compare-and-swap that target from the
evaluated integration base to the accepted artifact. Preflight rejects a
definition containing `workflows.promote` before creating a run or dispatching
any workflow. Qualified candidates remain durable for an external,
target-specific promotion boundary. That boundary must verify the exact
artifact and integration base against the evaluation, perform an atomic
integration check, and require re-evaluation when either has changed.
Successful development tests alone do not establish acceptance or promotion.

## Local revisions and outcomes

Updates carry `base_revision` for compare-and-swap protection. The control boundary
supplies caller identity and update authority separately from the request file.
Evaluator, objective, and comparison changes need explicit evaluator authority.
Restriction changes need explicit restriction authority.

Updates are pending until acknowledged. Changed action restrictions require
affected work to be paused before acknowledgment; otherwise the update remains
pending. Acknowledgment records relevant prior actions without claiming
retroactive prevention. Rejected and superseded updates remain distinguishable.
The host must persist activation and history before announcing the new revision.

The CLI publishes complete pending requests atomically. A running owner checks
the inbox after development and evaluation complete, before candidate acceptance.
It revalidates the base revision, authority and immutable evaluator snapshot,
persists activation through the existing journal/store, then regrades both the
retained incumbent and in-flight candidate under the new revision. A still-qualified
incumbent remains the comparator, so a revision change cannot turn a regression
into an initial qualification. A request arriving during regrading is checked
again before acceptance. Invalid requests are durably rejected; they never become
active. Stopped runs can still activate requests through explicit safe resume.
Uncertain in-flight work requires reconciliation. This unsandboxed host rejects
unsupported action restrictions instead of claiming that it enforced them.

Live activation moves the previous accepted result into history and clears the
current accepted slot in the same durable update. Only successful requalification
restores it. Failed or budget-exhausted regrading therefore leaves no stale
accepted result in the active journal or subsequent workflow triggers.

This initial contract conservatively requires full re-evaluation after any
Requirements Revision. Older accepted artifacts remain recoverable history;
stale evidence cannot qualify them for the current outcome.

Completion is separate from incremental acceptance and the stopping reason.
Objective targets must hold for every repeated sample. Additional completion
checks may require a human judgment. An empty work queue does not prove completion.
`cycle_complete` means a requested single cycle finished; it does not imply a
resource limit or completion. Operational failure and cancellation retain their
own stopping reasons.

The local driver distinguishes a limit reached between phases from a deadline
expired during work. Both report `resource_limit`; uncertain external effects
retain an ownership marker and require reconciliation. SIGINT reports `cancelled`,
never completion. In-flight cancellation has the same reconciliation requirement.
Counters currently measure workflow-role dispatches, including repeated grading,
not every nested operator/tool retry. Token, cost, and nested retry limits are not
hard enforcement guarantees of this adapter.

The outcome reports active requirements, completion evidence, the qualifying
accepted result if present, historical result identities, blocked work, known
resource usage, and diagnostics. If none qualifies it reports `no acceptable
result found`, not impossibility. Resource counters include retries and repeated
evaluation. Token and monetary values are not advertised as hard ceilings.

## Library integration

`newton_types::optimization` provides the wire types.
`newton_core::optimization` exposes definition parsing/binding, schema export,
candidate evaluation, promotion validation, revision handling, and outcome
construction. They do not introduce an HTTP endpoint or a second execution engine.
The existing native driver owns execution, durable state, isolation, resource
enforcement, and local control transport.

### Same-process observation

An embedding host can call
`newton_cli::cli::commands::optimize::observation::ObservedOptimizationRun::start`
with an authorized bound definition, definition directory, state directory, and
event capacity (1–4096). This uses the normal preflight, immutable input snapshot,
ownership, and native driver. It returns the runnable driver and a separate
read-only `OptimizeRunObservation`; it starts no server or background execution.
Call the driver's `run` method while consuming the observer's `next_update`.

`snapshot()` supplies the initial Run/Cycle trajectory. Subsequent updates contain
only that Run's durable snapshot. Subscription precedes the initial read so a
concurrent update is not lost. Channel overflow returns `lag_recovered` with a
fresh stored trajectory; it does not claim every intermediate event was retained.
`refresh()` explicitly reloads state. Snapshot reads retry concurrent Run changes
four times before returning a retryable error, and cost O(recorded Cycles).
Dropping an observer never cancels work. Channel closure is not completion;
completion comes from the saved outcome.

The observer uses the exact store and publisher owned by its driver in the same
process. The core `OptimizeRunObservationSource` also composes those capabilities
for an embedding host. A separate `newton serve` process can read persisted state
but does not receive this in-process event channel. Hosts must authorize execution
and Run disclosure before embedding; existing HTTP authentication and exposure
rules are unchanged. No HTTP start, stop, or run-edit endpoint is added.

## Opt-in real-agent gate

Configure a disposable Rust repository with a remediable dependency finding and
the shipped definition, using `parameter.agent=pi` and your existing Pi/gateway
configuration. Then run:

```sh
python3 scripts/test-optimize-live.py /path/to/workspace default ./target/debug/newton
```

The harness uses Newton's existing AgentOperator → aikit → Pi path. It checks
preflight, actual development/regrading, a qualifying changed commit, and an
unchanged original HEAD. It rejects a clean-baseline run as insufficient evidence
of agent execution. Credentials are neither collected nor injected by the harness.
This tier is opt-in and must be reported as unrun when its configuration is absent.

## Optional status projection

By default, optimization runs without a tracker. To reflect a run's terminal
status onto an existing GitHub Project item, create `.newton/projections.json`
before starting the run:

```json
{
  "schema_version": 1,
  "max_delivery_attempts": 8,
  "timeout_seconds": 5,
  "destinations": [
    {
      "name": "engineering-board",
      "binding": {
        "kind": "github_project_item",
        "project_id": "PVT_REPLACE_WITH_PROJECT_ID",
        "item_id": "PVTI_REPLACE_WITH_EXISTING_ITEM_ID",
        "field_id": "PVTSSF_REPLACE_WITH_STATUS_FIELD_ID",
        "status_options": {
          "converged": "REPLACE_WITH_DONE_OPTION_ID",
          "cycle_complete": "REPLACE_WITH_REVIEW_OPTION_ID",
          "no_actionable_work": "REPLACE_WITH_STOPPED_OPTION_ID",
          "no_progress": "REPLACE_WITH_STOPPED_OPTION_ID",
          "regressed": "REPLACE_WITH_BLOCKED_OPTION_ID",
          "stalled_on_blocked": "REPLACE_WITH_BLOCKED_OPTION_ID",
          "resource_limit": "REPLACE_WITH_STOPPED_OPTION_ID",
          "cancelled": "REPLACE_WITH_STOPPED_OPTION_ID",
          "failed": "REPLACE_WITH_BLOCKED_OPTION_ID"
        }
      }
    }
  ]
}
```

Replace every placeholder with an existing authorized identity. The configured
execution authority must include `command`, `network`, and `publish`, and existing
`gh` authentication must permit editing that item. Newton does not create
issues/items, search issue text for identity, or read the board to decide work.

The run freezes configuration before work; later source edits do not change its
destination on resume. This includes no-tracker mode: adding the file after a run
starts does not silently enable external writes for that run.

After the internal outcome is persisted, bounded status assignment runs
separately. Tracker outages and timeouts leave a durable pending assignment;
repeating synchronization of the same terminal outcome retries it without
creating another item. Missing status-option mappings are reported, not guessed.
Delivery reports live in `projection-report.json` beside the Run journal;
only pre-execution configuration diagnostics are frozen in the journal itself.
`newton optimize <PROJECT> --resume <RUN_ID>` retries projection for a finished
run before printing its saved outcome. Delivery and retry write only projection
metadata; they do not claim optimization ownership, edit the saved outcome, or
restart work. Human board edits cannot approve work or change Newton's accepted
result.

Map the native `resource_limit` status for any exhausted cycle, work, evaluation,
or elapsed-time budget. A stopped board item does not imply verified completion.
The historical `max_cycles` status is not the native resource-stop key.

This initial integration projects terminal Run status. Automatic item creation,
per-Plan/Change-Request lifecycle delivery, and a background retry daemon are not
included. No daemon is needed for a basic local run.
