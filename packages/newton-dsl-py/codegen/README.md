# Codegen scripts

These scripts regenerate `src/newton/_generated/ir.py` from the shared workflow
schema artifact at `packages/workflow-schema/workflow.schema.json`.

## Regenerate after schema changes

```bash
cd packages/newton-dsl-py
bash codegen/generate.sh
```

## CI drift check

Fails if the committed `ir.py` differs from a fresh generation:

```bash
bash codegen/check_drift.sh
```

## How it works

1. `newton schema export` (implemented in spec 057) produces the composed
   JSON Schema at `packages/workflow-schema/workflow.schema.json`.
2. `generate.sh` runs `datamodel-codegen` over that file to produce pydantic
   v2 models as `src/newton/_generated/ir.py`.
3. The generated file is committed to version control so the package works
   without running codegen at install time.
4. `check_drift.sh` is run in CI to ensure the committed file matches the
   current schema. If it drifts, run `generate.sh` and commit the update.
