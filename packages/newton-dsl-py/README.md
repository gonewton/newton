# newton-dsl (Python)

A Python authoring surface for [Newton](../../README.md) workflow graphs. Write
workflows in Python, compile to the YAML IR that `newton run` executes.

The engine never learns the workflow came from Python — it reads the same YAML
opcode whether you wrote it by hand or compiled it with this library
([ADR 0005](../../docs/adr/0005-authoring-surfaces-compile-to-one-provenance-blind-ir.md)).

## Quick start

```python
from newton import Workflow, command, agent, when, expr

wf = Workflow(
    "my-workflow",
    default_engine="codex",
    allow_shell=True,
    max_workflow_iterations=10,
)
wf.inputs(prompt="")

t1 = wf.task("run_check", command("./scripts/check.sh", shell=True, capture_stdout=True))
t2 = wf.task("fix_issues", agent(prompt=expr('context.preamble + tasks.run_check.output.stdout')))
t3 = wf.finish("done")

t1.then(t3, when=expr('contains(tasks.run_check.output.stdout, "OK")')).then(t2)
t2.then(t1)           # loop back — make sure t1 has repeat_at_most!
t1.repeat_at_most(5)

print(wf.to_yaml())   # emits validated YAML
```

## The `planning_enriching` workflow, side by side

Below is an excerpt from the `planning_enriching` workflow showing how the Python
DSL maps to the underlying YAML IR.

### Python (DSL)

```python
from newton import Workflow, command, agent, expr
from newton.refs import AmbientRef

wf = Workflow(
    "planning-enriching",
    default_engine="codex",
    parallel_limit=1,
    max_time_seconds=999999999,
    max_task_iterations=3,
    max_workflow_iterations=15,
    allow_shell=True,
)
wf.inputs(prompt="", output_path="")
wf.expects("develop_primary_engine", "develop_primary_model")

engine = AmbientRef("develop_primary_engine")
model  = AmbientRef("develop_primary_model")

enrich_spec = wf.task(
    "enrich_spec",
    agent(
        engine=engine,
        model=model,
        prompt=expr(
            'context.preamble + "\\n\\n...\\n\\n" + triggers.prompt'
        ),
    ),
)
enrich_spec.repeat_at_most(2)

check_gaps = wf.task(
    "check_gaps",
    command(
        'if grep -q "NEED_USER_INPUT" "$OUT"; then printf "has_gaps"; else printf "no_gaps"; fi',
        shell=True,
        env={"OUT": wf.input.output_path},
        capture_stdout=True,
    ),
)

cat_gaps  = wf.task("cat_gaps",  command("cat .newton/plan/gaps.txt 2>/dev/null || echo 'none'", shell=True, capture_stdout=True))
finalize  = wf.task("finalize",  command("echo Enriched spec written to $OUT", shell=True, env={"OUT": wf.input.output_path}))
finalize._terminal = "success"

enrich_spec.then(check_gaps)
check_gaps.then(cat_gaps,  when=expr('tasks.check_gaps.output.stdout == "has_gaps"')) \
          .then(finalize,  when=expr('tasks.check_gaps.output.stdout == "no_gaps"'))
cat_gaps.then(...)   # ... continues

print(wf.to_yaml())
```

### Compiled YAML (excerpt)

```yaml
version: '2.0'
mode: workflow_graph
metadata:
  name: planning-enriching
triggers:
  type: manual
  schema_version: '1.0'
  payload:
    prompt: ''
    output_path: ''
workflow:
  settings:
    entry_task: enrich_spec
    max_time_seconds: 999999999
    parallel_limit: 1
    max_task_iterations: 3
    max_workflow_iterations: 15
    default_engine: codex
    command_operator:
      allow_shell: true
  context: {}
  tasks:
    - id: enrich_spec
      operator: AgentOperator
      max_iterations: 2
      params:
        engine:
          $expr: develop_primary_engine
        model:
          $expr: develop_primary_model
        prompt:
          $expr: 'context.preamble + "\n\n...\n\n" + triggers.prompt'
      transitions:
        - to: check_gaps
    - id: check_gaps
      operator: CommandOperator
      params:
        shell: true
        env:
          OUT:
            $expr: triggers.output_path
        cmd: 'if grep -q "NEED_USER_INPUT" "$OUT"; then ...; fi'
        capture_stdout: true
      transitions:
        - to: cat_gaps
          priority: 0
          when:
            $expr: tasks.check_gaps.output.stdout == "has_gaps"
        - to: finalize
          priority: 5
          when:
            $expr: tasks.check_gaps.output.stdout == "no_gaps"
    - id: finalize
      operator: CommandOperator
      params:
        shell: true
        env:
          OUT:
            $expr: triggers.output_path
        cmd: echo Enriched spec written to $OUT
      terminal: success
```

## Key design decisions

| Concept | Design |
|---------|--------|
| **`when=` only, never `if`** | Conditions are values passed to `.then(when=...)`, not Python `if` branches. Structurally prevents parse-time/run-time confusion. |
| **Priority by declaration order** | First `.then()` call gets priority 0, next gets 5, 10, … `.otherwise()` is the unconditional fallback (lowest priority). |
| **Loop = back-edge + cap** | `a.then(b)` + `b.then(a)` + `a.repeat_at_most(5)`. No `while` blocks — real workflows have overlapping, non-nested cycles. |
| **`t.out.field`** | `__getattr__` returns an `OutRef` that renders as `{"$expr": "tasks.<id>.output.<field>"}` in YAML. Rename the task once and every reference follows. |
| **`t.output`** | Returns a `TaskOutputRef` for the entire task output object (`tasks.<id>.output`). Use when passing the whole output to another operator. |
| **Typed scopes** | `wf.input.x` → `triggers.x`; `wf.context.x` / `wf.var.x` → `context.x`; `wf.env("VAR")` → `env("VAR")`; `AmbientRef("name")` → injected ambient var. |

## Compiler checks

The compiler runs **before** YAML serialization and catches:

| Check | Behaviour |
|-------|-----------|
| **Dangling edge** | `CompilerError` — transition to undefined task |
| **Unbounded cycle** | `CompilerError` — cycle with no `repeat_at_most(N)` in it |
| **Unreachable task** | `UserWarning` — task not reachable from entry_task (e.g. dead `task-1` in `planning_enriching`) |
| **Unknown `.out.field`** | `CompilerError` (when operator schema is known) |

## Layer 1 — generated pydantic models

`src/newton/_generated/ir.py` is generated from the shared schema artifact
`packages/workflow-schema/workflow.schema.json` via `datamodel-codegen`.

To regenerate after a schema change:

```bash
cd packages/newton-dsl-py
bash codegen/generate.sh
```

To check for drift in CI:

```bash
bash codegen/check_drift.sh
```

## Running tests

```bash
cd packages/newton-dsl-py
uv run pytest
```
