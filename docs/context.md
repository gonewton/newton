# Ubiquitous Language — Newton

Newton is a workflow-first CLI and web platform for deterministic automation and software portfolio
management. It runs structured, repeatable workflows defined in YAML; maintains a portfolio model
(Products → Components → Repos → Modules) with evaluation history; and provides a UI for
real-time monitoring, human-in-the-loop gating, opportunity triage, and plan approval.

---

## Portfolio hierarchy

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Product** | The top of the portfolio hierarchy. A business-level product or service that groups **Components** under a common ownership boundary. | Service, project |
| **Component** | A bounded technical system owned by a team, belonging to one **Product**. Carries `owner`, **Criticality**, and **Autonomy** fields. Maps roughly to a microservice or platform. | Service, domain |
| **Repo** | A git repository belonging to a **Component**. Carries **Health** signals (`qualityScore`, `coverage`, `secScore`) and execution state. The unit most external tools (e.g., `dk review`) target directly. | Repository *(acceptable in Git contexts only)* |
| **Module** | A package or library inside a **Repo** (Rust crate, npm package, pip package, gem, jar). The lowest-granularity portfolio unit; used primarily for dependency tracking. | Package *(acceptable in language-specific contexts)*, library |
| **Portfolio** | An aggregated view linking a **Product** to its active **Plans** and **Executions**. | — |
| **Health** | A numeric score (0–100) aggregating quality signals at **Component** or **Repo** level. Below 65 is unhealthy; 65–79 is marginal; 80+ is healthy. | Score, rating |
| **Criticality** | Risk classification of a **Component**: `critical`, `high`, `medium`, `low`. Drives how Newton prioritizes planning and execution for that system. | Priority, severity |
| **Autonomy** | Governance level controlling how much Newton acts without human approval: `manual`, `supervised`, `assisted`, `autonomous`. Also called **Policy Level** in execution context. | Automation level |
| **Trend** | A directional indicator (`positive`, `negative`) showing whether a **Health** score is improving or declining between **EvalRuns**. | Delta direction |
| **SavedView** | A persisted filter and column configuration for portfolio or repository lists. | Filter preset, saved search |

---

## Evaluation model

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **KPI** | A catalog entry describing *what to monitor* — independent of any specific evaluation run. Has `threshold`, `weight`, `aggFn`, and `scopeLevel`. KPIs are few in number and change rarely. | Metric, indicator |
| **EvalRun** | A point-in-time evaluation event scoped to one entity level (`product`, `component`, `repo`, or `module`). Records `source`, optional `score`, `verdict`, and `summary`. Multiple EvalRuns per scope are expected — history is never overwritten. | Evaluation, report, scan |
| **Grade** | One evidence dimension within an **EvalRun**: a `score` in `[0, 100]`, optional `evidence` JSON, optional `kpiId` link, and optional `warnings`. A `(runId, dimension)` pair is append-only — once written it cannot be overwritten. | Score, dimension result |
| **Dimension** | The qualitative axis being graded within a **Grade** — e.g., `tests`, `security`, `coverage`. Distinguishes what aspect of quality is being measured, not just the numeric value. | — |
| **Evaluation Mode** | How a human-supplied **Grade** relates to a system-generated one: `complement` (adds a new dimension), `override` (replaces system score), or `train` (labeled example for model improvement). | — |

---

## Planning and improvement

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Opportunity** | A potential improvement linked to a **Component** or **Repo**. Has `risk`, `expectedValue`, **Effort**, **Origin**, and `status` (`awaiting_triage → triaged → approved_for_planning → structured → deferred/rejected`). | Issue, suggestion, recommendation |
| **Effort** | T-shirt sizing for an **Opportunity**: `XS`, `S`, `M`, `L`, `XL`. | Story points, complexity |
| **Origin** | Whether an **Opportunity** was surfaced by the system automatically or submitted by a human. Values: `system`, `human`. | Source, provenance |
| **Regression** | A detected deterioration in a **Repo**'s health signals. References a `kpiId`, carries `delta`, `severity`, and `trend`. | Degradation, decline |
| **Request** | A user-initiated work request that, if approved, becomes a **Plan**. Lifecycle: `draft → submitted → planning → rejected`. | Ticket, suggestion form |
| **Plan** | A structured set of actions addressing one or more **Opportunities**, composed of **PlanSections** and subject to **PlanPolicyChecks**. Lifecycle: `draft → awaiting_approval → approved → running → complete / needs_revision / rejected`. | Task file, ticket |
| **PlanSection** | An authored content subdivision within a **Plan** (e.g., "Background", "Proposed changes"). | — |
| **PlanPolicyCheck** | A governance validation rule attached to a **Plan**: `required`, `optional`, or `blocking`. Must pass before the plan can be approved. | Gate, check |
| **PlanApprover** | A named role that must approve a **Plan** before it proceeds. Tracks approval status per role. | Reviewer |
| **PendingApproval** | A dashboard item representing a **Plan** or **Execution** awaiting human review. Carries `risk`, `confidence`, and `expectedValue` for prioritization. | Action item |
| **RecentAction** | An activity feed entry reflecting a notable event: `agent`, `exec`, `approval`, `regression`, or `deferral`. | Audit entry, activity |

---

## Batch processing

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Batch Plan** | A Markdown file (with optional YAML frontmatter) in `.newton/plan/<project_id>/` that `newton batch` processes as a unit of work. Shares lifecycle vocabulary with a portfolio **Plan**. | Task file, job file |
| **Plan State** | The directory a **Batch Plan** resides in: `todo/` (ready), `completed/` (succeeded), `failed/` (errored), `draft/` (not yet ready). | Status folder |
| **Batch** | The operation of sequentially processing queued **Batch Plans** for a project by running the configured **Workflow** once per plan. Invoked via `newton batch <project_id>`. | Queue processing, bulk run |
| **BatchProjectConfig** | Per-project config at `.newton/configs/<project_id>.conf`: `{ project_root, workflow_file }`. Tells `newton batch` which **Workflow** to run against each plan. | — |

---

## Workflow definition

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Workflow** | A directed acyclic graph (DAG) of interconnected **Tasks**, defined in YAML. The primary unit Newton orchestrates. | Pipeline, job, script |
| **WorkflowDocument** | Root YAML container: `{ version, mode, macros, triggers, metadata, workflow }`. Version `2.0`, mode `workflow_graph`. | Config, spec file |
| **WorkflowDefinition** | Executable core nested inside a **WorkflowDocument**: `{ context, settings, tasks }`. | — |
| **WorkflowSettings** | Execution-control configuration: entry task, time limits, parallelism, checkpoint policy, completion policy, artifact storage, redaction. | Config, options |
| **Context** | A JSON object of global state threaded through all tasks. Seeded in the workflow definition; mutated at runtime by `SetContextOperator`. Available in expressions as `context.<key>`. | Variables, state bag |
| **Trigger Payload** | The JSON input to a workflow. Supplied via `--trigger KEY=VALUE` or `--parameters-json <file>`. Available in expressions as `triggers.payload`. | Parameters, inputs, args |
| **IoBlock** | The workflow's I/O contract: `{ input_schema, output_schema, result_map, error_schema }`. Validates **Trigger Payload** shape and maps final outputs. | Schema, contract |
| **Macro** | A named, reusable list of task templates defined at workflow level. Expanded inline when a **MacroInvocation** references them. | Template, include |
| **MacroInvocation** | A reference to a **Macro** with optional parameter substitution: `{ macro: name, with: { key: value } }`. | Macro call, use |

---

## Tasks

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Task** | A single unit of work in a workflow DAG. Has an `id`, an `operator`, `params`, optional **Transitions**, a **RetryPolicy**, and a `timeout_ms`. | Step, action, node |
| **Operator** | A pluggable executor that carries out a task. Receives `params` and **ExecutionContext**; returns a JSON value. | Handler, runner, plugin |
| **OperatorRegistry** | The registry mapping operator names to `Operator` implementations. Built once before each workflow run. | — |
| **Transition** | A directed edge from one task to another: `{ to, when?, include_if?, priority, label? }`. Governs routing after a task completes. | Edge, link, arrow |
| **Condition** (`when` / `include_if`) | A Rhai expression guard on a **Transition** or task inclusion. `include_if` skips the task; `when` skips only the edge. | Guard, filter |
| **Entry Task** | The first task executed in a workflow. Defaults to `"start"`; overridden via `settings.entry_task`. | Start node |
| **Parallel Group** | An optional tag grouping tasks that may run concurrently, bounded by `settings.parallel_limit`. | Thread group, fork |
| **RetryPolicy** | Per-task retry configuration: `{ max_attempts, backoff_ms, backoff_multiplier, jitter_ms }`. Applied on transient failures before the task is marked failed. | Retry config |
| **Goal Gate** | A task marked `goal_gate: true`. When `settings.completion.require_goal_gates` is enabled, at least one goal gate must succeed for the **Execution** to complete. | Milestone, checkpoint task |
| **Goal Gate Group** | An optional label grouping related **Goal Gates**. All tasks in the same group must succeed for the group to be satisfied. | — |
| **Terminal Task** | A task marked `terminal: success` or `terminal: failure`. Reaching it immediately resolves the **Execution**, bypassing further tasks. | End task, exit node |

---

## Execution

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Execution** | A single run of a workflow, identified by `execution_id` (UUID). Records status, start/end times, task outcomes, and warnings. Persisted at `.newton/state/workflows/<execution_id>/execution.json`. The backend data model term. | Run *(acceptable only in CLI `--run-id` context)*, instance |
| **Instance** | The UI/API surface term for an **Execution**. A `WorkflowInstance` carries `instance_id`, status, and `NodeStates`. Prefer **Execution** in backend/data contexts; **Instance** is acceptable in UI and WebSocket event contexts. | — |
| **Execution ID** | UUID that uniquely identifies an **Execution**. The CLI flag `--run-id` and the UI field `instance_id` are aliases for the same value. | Run ID *(CLI alias only)* |
| **ExecutionStatus** | The outcome of a workflow run: `Running`, `Completed`, `Failed`, `Cancelled`. The UI additionally uses `paused` as a display state. | State, result |
| **Stage** | A human-readable phase label attached to an **Execution** in the UI (e.g., "Testing", "Deployment"). Derived from active task context; not stored in the core execution record. | Phase, step |
| **NodeState** | The UI/API term for the state of a single **Task** within an **Instance**: `pending`, `running`, `succeeded`, `failed`, `timeout`, `cancelled`. Corresponds to **TaskStatus** in backend code. | Task state |
| **TaskOutcome** | The backend data structure holding the result of running a single task: task ID, context patch, success/failure flag, timing, error summary, and resolved params. | Task result |
| **Task Run Sequence** (`run_seq`) | Monotonically increasing counter for attempt iterations on a single task within one execution. Incremented per retry. | Attempt number |
| **StateView** | Immutable snapshot of `{ context, tasks, triggers }` passed to operators and expressions. Represents the world at the moment a task fires. | Snapshot |
| **ExecutionContext** | Per-task runtime container: workspace path, `execution_id`, `task_id`, current iteration, **StateView**, **OperatorRegistry**, nesting depth, and overrides. Passed into every `Operator::execute` call. | Runtime context, operator context |
| **Nesting Depth** | Integer tracking sub-workflow recursion level (0 = root). Prevents unbounded recursion when `WorkflowOperator` spawns child workflows. | — |
| **ExecutionTestResult** | Test outcome for an execution visible in the Execution Center: `passed`, `failed`, `running`. | — |

---

## Checkpointing and durability

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Checkpoint** | A persisted execution snapshot that enables **Resume**. Stored at `.newton/state/workflows/<execution_id>/checkpoint.json`. | Savepoint, snapshot |
| **WorkflowCheckpoint** | The checkpoint data structure: `{ execution_id, workflow_hash, ready_queue, context, trigger_payload, task_iterations, completed, runtime_tasks, io_snapshot }`. | — |
| **Ready Queue** | The list of task IDs queued to execute at checkpoint time. Restored verbatim on **Resume**. | Work queue, pending tasks |
| **Resume** | Restarting a workflow from its last **Checkpoint** via `newton workflow resume --run-id <UUID>`. | Restart, retry run |
| **Artifact** | Task output too large to store inline. Persisted to `.newton/artifacts/` with a SHA-256 hash and a path reference. | File output, blob |
| **OutputRef** | A discriminated union: `Inline(Value)` for small outputs, or `Artifact { path, size_bytes, sha256 }` for large ones. | — |
| **Audit Log** | Append-only `.jsonl` file recording all human-in-the-loop interactions for compliance. Written at `.newton/state/workflows/<execution_id>/audit.jsonl`. | Interaction log |

---

## Human-in-the-loop (HIL)

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **HIL Event** | A human intervention request raised by a **HumanApprovalOperator** or **HumanDecisionOperator**. Has `event_type` (`question` or `authorization`) and `status` (`pending`, `resolved`, `timedout`, `cancelled`). | Approval request, prompt |
| **HIL Action** | A human's response to a **HIL Event**: `text`, `authorization_approved`, `authorization_denied`, `timeout`, or `cancelled`. | Response, answer |
| **HumanApprovalOperator** | An **Operator** that pauses the workflow to request a boolean approve/deny decision. Output: `{ approved, reason, timestamp }`. | Approval gate |
| **HumanDecisionOperator** | An **Operator** that pauses the workflow to request a multiple-choice or text response from a human. Output: `{ choice, label, timestamp, timeout_applied, default_used }`. | Decision gate |
| **Timeout Behavior** | If a human does not respond within `timeout_seconds`, the operator returns `default_choice` if configured; otherwise fails with `WFG-HUMAN-105`. | — |

---

## Operators (built-in)

| Term | Definition |
| --- | --- |
| **CommandOperator** | Executes a shell command; captures stdout/stderr as a JSON value. |
| **SetContextOperator** | Deep-merges a JSON patch into the workflow **Context**. |
| **NoOpOperator** | Pass-through task for routing or branching without side effects. |
| **WorkflowOperator** | Runs a nested workflow from another YAML file in-process, incrementing **Nesting Depth**. |
| **BarrierOperator** / **AssertCompletedOperator** | Synchronization tasks that block until a specified set of task IDs have completed. |
| **HumanApprovalOperator** | See *Human-in-the-loop* section above. |
| **HumanDecisionOperator** | See *Human-in-the-loop* section above. |
| **AgentOperator** | Runs an AI agent engine via aikit-sdk with **Signal**-based output routing. Supports checkpoint/resume. |
| **GhOperator** | Wraps the GitHub CLI for PR and project operations. Supports checkpoint/resume. |
| **ReadControlFileOperator** | Reads and parses a JSON file at a runtime-resolved path into the task output. |

---

## Agent execution

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Engine** | The AI backend for an **AgentOperator** task: `claude`, `codex`, `gemini`, `opencode`, etc. Set per-task or via `settings.default_engine`. | Model, provider |
| **Signal** | A named mapping from an agent output event (`success`, `failure`, `timeout`, `invalid_output`) to a target **Transition**. Routes the workflow based on agent behavior. | Callback, hook |
| **ModelStylesheet** | Workflow-level agent model configuration: `{ model, context_fidelity }`. Applies to all agent tasks unless overridden per task. | Model config |
| **ContextFidelity** | How much conversation history the agent retains: `Full`, `Summary`, `Truncate`. | Memory mode |

---

## Expressions

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Expression** | A Rhai script embedded in YAML to compute dynamic values or conditions. Evaluated against `{ context, tasks, triggers }`. | Script, formula |
| **`$expr`** | YAML marker indicating the field value is an **Expression**: `{ $expr: "context.version == 'v1'" }`. | — |
| **Template Interpolation** | `{{ expr }}` syntax in string fields; expands the expression result inline. | String templating |

---

## Real-time and UI

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **ConnectionStatus** | The state of the WebSocket/SSE connection to the Newton server: `disconnected`, `connecting`, `connected`, `reconnecting`. | Socket state |
| **BroadcastEvent** | A WebSocket notification of a server-side change. Carries only IDs (not full payloads); the UI re-fetches via REST on receipt. Types: `workflowInstanceUpdated`, `nodeStateChanged`, `logMessage`, `hilEvent`. | Push event, socket message |
| **WorkflowMonitor** | The three-pane UI surface: instance list (left), DAG graph (center), task detail (right). Unifies monitoring, log viewing, and HIL handling. | Dashboard, viewer |
| **Magic Button** | A UI pattern where AI generates a draft proposal that a human reviews before applying. The draft is never auto-applied. See also **DraftCard**. | AI autocomplete, auto-fill |
| **DraftCard** | The review surface for a **Magic Button** proposal. Shows AI-generated content alongside an accept/reject affordance. | — |

---

## Diagnostics

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Lint** | Static analysis of a workflow file against best-practice rules. Produces findings with severity `Error`, `Warning`, or `Info`. | Validate, check |
| **Error Code** | A prefixed string identifying a specific failure: `WFG-*` (workflow graph), `HIL-*` (human-in-the-loop), `WFG-EXPR-*` (expressions), `WFG-TIME-*` (timeouts), `WFG-AGENT-*` (agent). | Error key |

---

## Workspace

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Workspace** | The directory containing a `.newton/` folder. Newton discovers it by walking up from the current directory. | Project root, working dir |
| **`.newton/`** | Workspace root. All Newton state, configs, plans, artifacts, and logs live here. | Newton dir, state dir |

---

## Dependency mapping

> Status: draft — terms being resolved in spec `056-dependencies-crate`.

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Dependency** | A directed link meaning "this artifact relies on that one": `from → to`. The unit of reasoning is the **Module**, though links may also be recorded at **Repo** level. Carries a **Discovery** and (where the version scheme allows) a version constraint. | Edge, link, reference |
| **Discovery** | How a **Dependency** became known: `Detected` (a machine read it from a real manifest/lockfile), `Declared` (a person stated it), or `Suggested` (analysis or AI proposed it — must be reviewed before it is trusted). One stored value per dependency. | Provenance, source, origin |
| **Discovery Process** | The automated pass that reads manifests/lockfiles across repos and produces `Detected` dependencies. At **Module = crate** granularity it captures most cross-repo *crate-level* edges (registry/git/path deps name other Modules, including in other repos). What it cannot see is **non-package** dependencies — cross-service API calls, shared schemas, runtime/contract deps — which must be supplied as `Declared`. | Scan, import |
| **Baseline** | A trusted **Dependency** map: the `Detected` edges from the **Discovery Process** plus the human-`Declared` edges added to fill what discovery cannot see, blessed once before planner agents rely on it. Re-running the Discovery Process refreshes `Detected` edges but never drops `Declared` ones. | Snapshot, trusted graph |
| **Confirmed** | A *derived* yes/no flag on a **Dependency**: true when **Discovery** is `Detected` or `Declared`, false when `Suggested`. Only **Confirmed** dependencies drive release sequencing automatically; `Suggested` ones are surfaced for human promotion first. | Trusted, verified |
| **Impact Sequence** | The computed, ordered list of **Modules** that must be re-released to carry a change from a modified Module up to a named **Target**. Propagation-driven, not breakage-driven: a Module is included if it lies on a dependency path to the Target, *even if it could technically absorb the change* — because the Target only receives the change if every hop is re-released. An output, not a stored entity. Consumed by an agent to author **Plans**. | Release plan, release order, blast radius |
| **Target** | The boundary that scopes an **Impact Sequence** — typically the **Product** being worked on. Propagation only follows dependency paths that reach the Target, preventing the ripple from expanding to unrelated dependents across the portfolio. | Goal, destination |
| **Effort Class** | A per-hop label on each Module in an **Impact Sequence** describing how much work its update is: `bump-only` (the change is compatible — mechanical re-pin/rebuild/release), `adapt` (the change is breaking — needs code changes first), or `unknown` (no **Compatibility Signal** available — treated as `adapt`). | Difficulty, size |
| **Compatibility Signal** | Whether a change to a Module breaks what its consumers expect: `breaking`, `non-breaking`, or `unknown`. The input that drives **Effort Class**. Each project owns its own version scheme and bumping — Newton never assigns versions. The signal is *derived* from the version delta only when the project's scheme encodes compatibility (e.g. semver major vs minor); for schemes that don't (commit SHA, calver), it must be *stated* by whoever initiates the change, defaulting to `unknown` when absent. | Version bump, breaking change |
| **Co-release Group** | A strongly-connected set of Modules (a dependency cycle, common at crate level when modules of two repos depend on each other). A plain release order is undefined across it. The analysis surfaces it explicitly — never silently ordering or failing — for the planner to resolve, either by **breaking the cycle temporally** (one edge depends on the already-*released* version, not the new one) or by **coordinating the group** as a single release unit. | Cycle, deadlock |

---

## Relationships

- A **Product** contains one or more **Components**; a **Component** contains one or more **Repos**; a **Repo** contains zero or more **Modules**.
- An **EvalRun** is scoped to exactly one entity level and contains one or more **Grades**; each **Grade** optionally references one **KPI**.
- A **Regression** is associated with one **Repo** and optionally one **KPI**.
- An **Opportunity** is linked to one **Component** or **Repo** and may reference one **KPI**.
- A **Request**, if approved, produces a **Plan**; a **Plan** triggers one **Execution** of the configured **Workflow**.
- A **Batch Plan** file also produces exactly one **Execution**; on success it moves to `completed/`, on failure to `failed/`.
- A **Workflow** contains one **WorkflowDefinition** with one or more **Tasks**; each **Task** is executed by one **Operator**.
- A **Task** has zero or more **Transitions** to other tasks; **Conditions** guard which transitions fire.
- An **Execution** accumulates one **TaskOutcome** per completed task and one **WorkflowCheckpoint** per checkpoint interval.
- A **Goal Gate** is a **Task**; **CompletionSettings** determine how many must succeed before the **Execution** is `Completed`.
- An **HumanApprovalOperator** or **HumanDecisionOperator** task raises one **HIL Event**; the human submits exactly one **HIL Action** to resolve it.

---

## Example dialogue

> **Dev:** "A `dk review` run finished on `newton-backend`. Where does that land in the model?"
>
> **Domain expert:** "It creates an **EvalRun** scoped to that **Repo**, then one **Grade** per **Dimension** — `tests`, `security`, `coverage`. Each **Grade** links back to the matching **KPI** so we can track **Health** over time. Nothing is overwritten; history accumulates."
>
> **Dev:** "If coverage drops next week, does that create a **Regression** automatically?"
>
> **Domain expert:** "Yes — Newton compares the new **EvalRun** score against the **KPI** threshold and flags a **Regression** with the `delta` and `trend`. That may surface an **Opportunity** in `awaiting_triage` for the owning team."
>
> **Dev:** "When the team decides to act, what's the flow?"
>
> **Domain expert:** "They triage the **Opportunity** to `approved_for_planning`. Newton structures a **Plan** — sections, **PlanPolicyChecks**, and **PlanApprovers**. Once all approvers sign off, the plan moves to `approved` and triggers an **Execution** of the configured **Workflow**."
>
> **Dev:** "The workflow has a step where a human needs to approve a PR. How does that work in the UI?"
>
> **Domain expert:** "The **AgentOperator** opens the PR, then a **HumanApprovalOperator** task raises a **HIL Event**. The **WorkflowMonitor** shows the **Instance** as `paused`; the reviewer opens the **HIL Panel**, sees the **HIL Event**, and submits a **HIL Action** — `authorization_approved` or `authorization_denied`. That resolves the event and the workflow resumes."
>
> **Dev:** "If the machine dies mid-run, is progress lost?"
>
> **Domain expert:** "No — each **Checkpoint** persists the **Ready Queue** and **Context**. `newton workflow resume` reloads the **WorkflowCheckpoint** and continues from where it stopped. **Goal Gates** that already succeeded are preserved."

---

## Flagged ambiguities

- **"Execution" (backend) vs. "Instance" (UI)** — The backend data model uses `execution_id` and `WorkflowExecution`; the UI and WebSocket API use `instance_id` and `WorkflowInstance`. They refer to the same entity. Prefer **Execution** in backend/data/CLI contexts; **Instance** is acceptable in UI and realtime event contexts. Never use both interchangeably in the same layer.
- **"Plan" (portfolio) vs. "Batch Plan" (file)** — The portfolio model uses **Plan** for a structured improvement proposal requiring approval; the batch subsystem uses **Batch Plan** for a Markdown file processed by `newton batch`. Both share `todo/completed/failed/draft` lifecycle vocabulary. Qualify as **improvement plan** or **batch plan** when the distinction matters.
- **"Run" vs. "Execution"** — The CLI exposes `--run-id` but the data model uses `execution_id`. Prefer **Execution** in code and documentation; `run-id` is a CLI display alias only.
- **"Checkpoint" (durability) vs. task-level sync** — Some teams call a validation task a "checkpoint". In Newton, **Checkpoint** always means the persisted execution snapshot used for **Resume**. Use **Goal Gate** or **Barrier** for task-level synchronization.
- **"Context" vs. "ExecutionContext"** — **Context** is the user-facing JSON state object available in expressions. **ExecutionContext** is the internal operator runtime struct. They are distinct; do not conflate them.
- **"Autonomy" vs. "Policy Level"** — The portfolio model names this field `autonomy`; the Execution Center table names it `policyLevel`. Same enum values (`manual`, `supervised`, `assisted`, `autonomous`), same concept. Prefer **Autonomy** in portfolio contexts, **Policy Level** in execution/governance contexts.
- **"Operator" (person) vs. "Operator" (plugin)** — Human-in-the-loop docs sometimes use "operator" for the human approving a task. Prefer **human operator** for the person, **Operator** (capitalized) for the plugin abstraction.
- **"NodeState" (UI) vs. "TaskStatus" (backend)** — Both describe the completion state of a task within a run. **TaskStatus** is the backend enum (`Success`, `Failed`, `Skipped`); **NodeState** is the UI/API representation (`pending`, `running`, `succeeded`, `failed`, `timeout`, `cancelled`) with a finer-grained set of values. Use the appropriate term for the layer you are working in.
- **"Release" is not a first-class entity** — Newton plans a ripple of changes but does not track "releases" as stored objects with their own lifecycle. The dependency model emits an **Impact Sequence** (a computed ordering of Modules to update), which the agent turns into existing **Plans** and **Executions**. Avoid using "Release" as a noun in the data model; say **Impact Sequence** for the ordering, **Plan**/**Execution** for the tracked work.
