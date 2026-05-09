# Newton CLI E2E Coverage Matrix

This document is the single source of truth mapping `(command path, flag) →
(test name, tier)` for the `newton` CLI E2E test suite (spec 301).

## Tiers

| Tier | Trigger | Wall-clock budget per test |
|---|---|---|
| smoke | implicit (every PR) | ≤ 2 s |
| integration | implicit (every PR) | ≤ 10 s |
| extended | `cargo test -- --ignored` (nightly) | ≤ 30 s |

## Root commands

The matrix gate enforces that every required root command id has at least one
smoke row below. The required set is the sixteen Newton ids registered in
`crates/cli/src/cli/framework_setup.rs`, plus the framework-provided `spec`
command.

Required smoke rows: `run`, `init`, `batch`, `serve`, `monitor`, `workflow`,
`resume`, `checkpoint`, `artifact`, `webhook`, `runs`, `health`, `doctor`,
`config`, `completion`, `ask`, `spec`.

## Coverage matrix

| Command path | Flag | Test name | Tier |
|---|---|---|---|
| run | --help | smoke_run_help | smoke |
| init | --help | smoke_init_help | smoke |
| batch | --help | smoke_batch_help | smoke |
| serve | --help | smoke_serve_help | smoke |
| monitor | --help | smoke_monitor_help | smoke |
| workflow | --help | smoke_workflow_help | smoke |
| resume | --help | smoke_resume_help | smoke |
| checkpoint | --help | smoke_checkpoint_help | smoke |
| artifact | --help | smoke_artifact_help | smoke |
| webhook | --help | smoke_webhook_help | smoke |
| runs | --help | smoke_runs_help | smoke |
| health |  | smoke_health | smoke |
| doctor | --help | smoke_doctor_help | smoke |
| config | --help | smoke_config_help | smoke |
| completion | --help | smoke_completion_help | smoke |
| ask | --help | smoke_ask_help | smoke |
| spec | --format json | smoke_spec_json | smoke |
| run | --workspace | integ_run_workspace_creates_state | integration |
| run | --trigger | integ_run_trigger_payload | integration |
| workflow validate |  | integ_workflow_validate_ok | integration |
| workflow lint | --format json | integ_workflow_lint_json | integration |
| workflow preview | --format text | integ_workflow_preview_text | integration |
| workflow graph |  | integ_workflow_graph_dot | integration |
| runs list | --workspace | integ_runs_list_seeded_workspace | integration |
| runs list | --json | integ_runs_list_json | integration |
| runs show | --workspace | integ_runs_show_seeded_run | integration |
| resume | --run-id | integ_resume_run_id | integration |
| checkpoint list | --json | integ_checkpoint_list_json_two_runs | integration |
| checkpoint clean | --older-than | integ_checkpoint_clean_older_than | integration |
| artifact clean | --older-than | integ_artifact_clean_removes_old | integration |
| init |  | integ_init_creates_workspace | integration |
| batch | --once | integ_batch_once_no_plans | integration |
| health |  | integ_health_command | integration |
| doctor |  | integ_doctor_command | integration |
| config show |  | integ_config_show | integration |
| completion | bash | integ_completion_bash | integration |
| serve | --port | ext_serve_ephemeral_port_health | extended |
| monitor | --help | ext_monitor_help_runs | extended |
| webhook serve | --workflow | ext_webhook_serve_starts | extended |
| ask |  | ext_ask_with_wiremock | extended |
| run | --bogus-flag (negative) | negative_run_unknown_flag | integration |
| workflow validate |  (missing positional) | negative_workflow_validate_missing_arg | integration |
| runs show |  (missing run id) | negative_runs_show_missing_id | integration |
| checkpoint clean |  (missing --older-than) | negative_checkpoint_clean_missing_older_than | integration |
| artifact clean |  (missing --older-than) | negative_artifact_clean_missing_older_than | integration |

## Performance

Baseline measured at PR open (newton-cli, smoke + integration tiers, excluding
`--ignored`):

- Baseline: 60 s (recorded at PR open via `cargo nextest run -p newton-cli`).
- Budget: baseline + 90 s = **≤ 150 s**.

The PR-tier wall-time MUST stay within this budget. `cargo nextest run --all-features --locked`
is the canonical measurement command (see `scripts/run-tests.sh`).

## Agent-facing CLI surface

`newton spec --format json` is the supported machine-readable export of the
public CLI surface for agents and future automation. The framework's
`command_surface::command::create_spec_command` produces a `CliSpecDocument`
containing fields like `schemaVersion`, `app`, and `commands`. The markdown
matrix above remains the human-reviewed source of truth for tier and test
mapping.
