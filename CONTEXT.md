# Newton — Context Glossary

The canonical language for Newton's CLI command surface. Terms here are domain
vocabulary, not implementation notes. When code and this glossary disagree, one
of them is wrong — resolve it.

## Terms

### Liveness
"Is the process alive and able to respond?" A cheap, dependency-free signal for
automation (load balancers, uptime polling). In Newton, liveness is an **HTTP
concern only** — exposed at the `/health` route by `serve`. It is *not* a CLI
command: a human at a shell wants a diagnostic, not a one-line pulse, and a
machine polling liveness already speaks HTTP. (The `health` CLI command is being
removed; its sole check folds into Doctor.)

### Doctor
The readiness/diagnostic surface. Human-facing, multi-probe (version, workspace
writability, config presence, ailoop reachability, `gh` on PATH, logging). May
grow probes over time. The *only* CLI entry point for "is my environment
healthy?" — any check liveness would have done belongs here instead.

### Step (a workflow run)
A single mutation pass over a project: one workflow graph, executed once. The
unit of work `workflow run` performs. A Step has no opinion about what runs
before or after it.

### Optimization loop
Newton's reason to exist (and the source of its name — Newton's method, iterating
toward an objective). The autonomous cycle that drives a project toward a better
**Grade**: `change-request → plan → refine → implement → test → grade →
change-request`. It is a **Driver** that sequences **Steps**; it is not itself a
Step. Distinct from a batch: a batch drains a finite set once and stops; the
optimization loop is closed — grading feeds the next round of change requests.
The CLI command is **`optimize`** (renamed from `batch`; see ADR 0003).
(As of this writing the loop is open: the grade→change-request edge is not yet
wired. The vocabulary describes the target.)

### Grade (the objective)
The north star the optimization loop maximizes — the objective function. A
project's current Grade is what every Step is ultimately trying to improve.

### Plan
A unit of queued work a project's optimization loop consumes — the spec for one
Step. This is the canonical noun; "work item", "task", and "batch item" are
deprecated synonyms to be removed.

### Plan queue
The per-project backlog of Plans and their lifecycle:
`todo → completed | failed` (with `draft`, `abandoned` as holding states). The
queue is the only durable state the loop owns between Steps.

### Driver
A way to set Steps running over the one execution engine. Newton has exactly
these drivers, distinguished only by what originates the work:
- **`workflow run`** — a human/CLI runs one Step, one shot.
- **`optimize`** — the autonomous Optimization loop drives Steps toward the Grade.
- **`serve`** — exposes the engine and its state over HTTP (observe).

External HTTP *ingress* (an outside system POSTing to start a Step) is **out of
scope**: Newton's optimizer is self-driving, not event-driven. See ADR 0004.

### ailoop (not a Driver)
Outbound, WebSocket-only human-in-the-loop: mid-Step, Newton reaches *out* to a
human monitor to ask a question. The opposite direction from ingress — a client
connecting out, never a producer of Steps. Frequently confused with webhooks; it
is unrelated.
