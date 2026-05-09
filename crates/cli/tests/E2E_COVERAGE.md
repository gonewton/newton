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
| run | --bogus-flag (negative) | negative_run_unknown_flag | integration |
| workflow validate |  (missing positional) | negative_workflow_validate_missing_arg | integration |
| runs show |  (missing run id) | negative_runs_show_missing_id | integration |

## Performance

Baseline measured at PR open. The PR-tier (smoke + integration, excluding
`--ignored`) wall-time MUST stay within baseline + 90 seconds.

## Agent-facing CLI surface

`newton spec --format json` is the supported machine-readable export of the
public CLI surface for agents and future automation. The framework's
`command_surface::command::create_spec_command` produces a `CliSpecDocument`
containing fields like `schemaVersion`, `app`, and `commands`. The markdown
matrix above remains the human-reviewed source of truth for tier and test
mapping.
