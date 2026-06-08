# @newton/dsl (TypeScript)

A TypeScript authoring surface for [Newton](../../README.md) workflow graphs. Write
workflows in TypeScript, compile to the YAML IR that `newton run` executes.

The engine never learns the workflow came from TypeScript — it reads the same YAML
opcode whether you wrote it by hand or compiled it with this library
([ADR 0005](../../docs/adr/0005-authoring-surfaces-compile-to-one-provenance-blind-ir.md)).

## Quick start

```ts
import { Workflow, command, agent, expr } from "@newton/dsl";

const wf = new Workflow("my-workflow", {
  defaultEngine: "codex",
  allowShell: true,
  maxWorkflowIterations: 10,
});
wf.inputs({ prompt: "" });

const t1 = wf.task("run_check", command({ cmd: "./scripts/check.sh", shell: true, captureStdout: true }));
const t2 = wf.task("fix_issues", agent({ prompt: expr('context.preamble + tasks.run_check.output.stdout') }));
const t3 = wf.finish("done");

t1.then(t3, { when: expr('contains(tasks.run_check.output.stdout, "OK")') });
t1.then(t2);
t2.then(t1);           // loop back — make sure t1 has repeatAtMost!
t1.repeatAtMost(5);

console.log(wf.toYaml());   // emits validated YAML
```

## The `planning_enriching` workflow, side by side

Below is an excerpt from the `planning_enriching` workflow showing how the TypeScript
DSL maps to the underlying YAML IR.

### TypeScript (DSL)

```ts
import { Workflow, command, agent, expr, AmbientRef } from "@newton/dsl";

const wf = new Workflow("planning-enriching", {
  defaultEngine: "codex",
  parallelLimit: 1,
  maxTimeSeconds: 999999999,
  maxTaskIterations: 3,
  maxWorkflowIterations: 15,
  allowShell: true,
});

wf.inputs({ prompt: "", output_path: "" });
wf.expects("develop_primary_engine", "develop_primary_model");

const engine = new AmbientRef("develop_primary_engine");
const model  = new AmbientRef("develop_primary_model");

const enrichSpec = wf.task(
  "enrich_spec",
  agent({
    engine,
    model,
    prompt: expr(
      'context.preamble + "\\n\\n...\\n\\n" + triggers.prompt'
    ),
  })
);
enrichSpec.repeatAtMost(2);

const checkGaps = wf.task(
  "check_gaps",
  command({
    cmd: 'if grep -q "NEED_USER_INPUT" "$OUT"; then printf "has_gaps"; else printf "no_gaps"; fi',
    shell: true,
    env: { OUT: wf.input.output_path },
    captureStdout: true,
  })
);

const catGaps  = wf.task("cat_gaps",  command({ cmd: "cat .newton/plan/gaps.txt 2>/dev/null || echo 'none'", shell: true, captureStdout: true }));
const finalize = wf.task("finalize",  command({ cmd: "echo Enriched spec written to $OUT", shell: true, env: { OUT: wf.input.output_path } }));
finalize._terminal = "success";

enrichSpec.then(checkGaps);
checkGaps
  .then(catGaps,   { when: expr('tasks.check_gaps.output.stdout == "has_gaps"') })
  .then(finalize,  { when: expr('tasks.check_gaps.output.stdout == "no_gaps"') });
// ... continues

console.log(wf.toYaml());
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
| **`{ when }` only, never `if`** | Conditions are values passed to `.then({ when: ... })`, not `if` branches. Structurally prevents parse-time/run-time confusion (ADR 0007). |
| **Priority by declaration order** | First `.then()` call gets priority 0, next gets 5, 10, … `.otherwise()` is the unconditional fallback (lowest priority). |
| **Loop = back-edge + cap** | `a.then(b)` + `b.then(a)` + `a.repeatAtMost(5)`. No `while` blocks — real workflows have overlapping, non-nested cycles. |
| **`t.out.field`** | `Proxy`-backed accessor returns an `OutRef` that renders as `{"$expr": "tasks.<id>.output.<field>"}` in YAML. |
| **`t.output`** | Returns a `TaskOutputRef` for the entire task output object (`tasks.<id>.output`). Use when passing the whole output to another operator. |
| **`ref.eq/.ne/.gt`** | TypeScript cannot overload `==`, so comparison methods return `Guard` objects. |
| **Typed scopes** | `wf.input.x` → `triggers.x`; `wf.context.x` / `wf.var.x` → `context.x`; `wf.env("VAR")` → `env("VAR")`; `new AmbientRef("name")` → injected ambient var. |

## Idiomatic differences vs Python surface (058)

| Concern | Python (058) | TypeScript (060) |
|---------|-------------|------------------|
| Conditions on edges | `when=` keyword | `{ when: ... }` options object |
| Typed output comparison | `review.out.passed == True` | `review.out.passed.eq(true)` / `.ne()` / `.gt()` |
| Naming | `repeat_at_most`, `default_engine` | `repeatAtMost`, `defaultEngine` (camelCase) |
| Operator sub-constructors | `gh.pr_create(...)` | `gh.prCreate(...)` |

## Compiler checks

The compiler runs **before** YAML serialization and catches:

| Check | Behaviour |
|-------|-----------|
| **Dangling edge** | `CompilerError` — transition to undefined task |
| **Unbounded cycle** | `CompilerError` — cycle with no `repeatAtMost(N)` in it |
| **Unreachable task** | `console.warn` — task not reachable from entry task |

## Layer 1 — generated TypeScript interfaces

`src/generated/ir.ts` is generated from the shared schema artifact
`packages/workflow-schema/workflow.schema.json` via `json-schema-to-typescript`.

To regenerate after a schema change:

```bash
cd packages/newton-dsl-ts
bash codegen/generate.sh
```

To check for drift in CI:

```bash
bash codegen/check_drift.sh
```

## Running tests

```bash
cd packages/newton-dsl-ts
pnpm test
```

## Conformance corpus

Both `newton-dsl-py` and `newton-dsl-ts` run against the shared conformance corpus at
`packages/workflow-schema/conformance/`. Adding a fixture once enforces parity across
both surfaces automatically.
