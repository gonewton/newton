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

Required smoke rows: `init`, `optimize`, `serve`, `workflow`,
`resume`, `checkpoint`, `artifact`, `runs`, `doctor`,
`config`, `completion`, `chat`, `spec`.

## Coverage matrix

| Command path | Flag | Test name | Tier |
|---|---|---|---|
| workflow run | --help | smoke_workflow_run_help | smoke |
| init | --help | smoke_init_help | smoke |
| optimize | --help | smoke_optimize_help | smoke |
| serve | --help | smoke_serve_help | smoke |
| workflow | --help | smoke_workflow_help | smoke |
| resume | --help | smoke_resume_help | smoke |
| checkpoint | --help | smoke_checkpoint_help | smoke |
| artifact | --help | smoke_artifact_help | smoke |
| runs | --help | smoke_runs_help | smoke |
| doctor | --help | smoke_doctor_help | smoke |
| config | --help | smoke_config_help | smoke |
| completion | --help | smoke_completion_help | smoke |
| chat | --help | smoke_chat_help | smoke |
| spec | --format json | smoke_spec_json | smoke |
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
| optimize | --once | integ_optimize_once_no_plans | integration |
| doctor |  | integ_doctor_command | integration |
| config show |  | integ_config_show | integration |
| completion | bash | integ_completion_bash | integration |
| workflow run | --bogus-flag (negative) | negative_run_unknown_flag | integration |
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

> **CI enforcement:** The budget is not yet automatically enforced in CI; it is
> measured manually on each PR. Track enforcement work in issue #301 follow-up.
> The nightly job (`ci-nightly.yml`) runs the extended tier but does not assert
> against this budget — it relies on the 20-minute job timeout as a coarse gate.

## Agent-facing CLI surface

`newton spec --format json` is the supported machine-readable export of the
public CLI surface for agents and future automation. The framework's
`command_surface::command::create_spec_command` produces a `CliSpecDocument`
containing fields like `schemaVersion`, `app`, and `commands`. The markdown
matrix above remains the human-reviewed source of truth for tier and test
mapping.
