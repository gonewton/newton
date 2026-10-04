# Newton configuration (`.newton/configs`)

Workspace Newton metadata lives under `.newton/`. This sheet describes the
**`key=value` `.conf` files** in `.newton/configs/` that the native `optimize`
command reads. Values are declarative bindings, not shell code.

Lines support `#` comments. Unknown keys are ignored by each consumer unless noted.

## `newton optimize <project_id>` — `.newton/configs/<project_id>.conf`

| Key | Meaning |
| --- | --- |
| `project_root` | Context root. It is absolute or relative to the Newton workspace. |
| `definition_file` | Versioned Optimization Definition YAML. `--definition` overrides it. |
| `optimize_allowed_actions` | Comma-separated authority ceiling: `agent`, `command`, `network`, `commit`, `draft_pull_request`, `publish`, `merge`, `deploy`. The host rejects a workflow it cannot enforce safely. |
| `parameter.<name>` | Ordinary JSON literal (or plain string) that overrides the definition default. It cannot grant permission. |

```text
# .newton/configs/myapp.conf
project_root=.
definition_file=.newton/definitions/security.yaml
optimize_allowed_actions=agent,command,network,commit,draft_pull_request,merge
parameter.test_command="cargo test --workspace"
```

The native driver does not consume Plan queues. It requires an evaluator and a
`propose` role; `execute` is optional. It persists binding, evaluations, attempts,
decisions, and reports as per-run JSON without requiring SQLite. Promotion is a
domain-specific step outside the generic loop. Use `--requirements-update` only
with `--resume`; an update cannot activate across uncertain in-flight effects.

---

## `newton init` — `.newton/configs/default.conf`

After `newton init .`, set `definition_file` only after choosing or authoring a
definition compatible with the installed template workflows. Initialization does
not infer a definition or grant workflow permissions.

---

## Model and engine strings

Valid model and engine identifiers depend on the **workflow YAML**, the **agent operator**, and your provider. Confirm with `newton workflow run --help` and your workflow definitions; do not treat examples in the wild as stable API.

---

## Validation errors (optimize)

Typical failures:

- Missing definition selection (`--definition` or `definition_file`).
- A malformed definition, unsupported restriction, or insufficient action authority.
- Candidate evidence that belongs to a different run, artifact, evaluator, or requirements revision.
- Observation selections outside the current assessment or above the configured K.
- An execute proposal without a configured `execute` workflow.
- A legacy schema-version-1 definition or pre-generic `journal.json` run.
- Unreadable `.conf` path (wrong `--workspace` or missing `.newton/configs/<project_id>.conf`).
