# Changelog

All notable changes to this project will be documented in this file.

## Unreleased

### Remove `newton monitor` subcommand (issue #303) — BREAKING CHANGE

The `newton monitor` CLI subcommand and its TUI implementation have been removed. Users who relied on `newton monitor` to interact with ailoop channels should use ailoop's own clients directly (for example `ailoop serve`, `ailoop ask`, and `ailoop say`). The `HumanApprovalOperator` and `HumanDecisionOperator` workflow operators continue to integrate with ailoop for in-workflow human gates.

### Align monitor docs with ailoop unified-port endpoint and migrate Newton skill in-tree (issue #298)

- Documentation, `newton monitor --help`, and monitor config-error messages now use the unified-port topology (`http://127.0.0.1:8080` and `ws://127.0.0.1:8080`) to match upstream ailoop's single-listener default.
- Monitor config error messages reference the canonical CLI flags `--ailoop-http` and `--ailoop-ws` instead of the legacy `--http-url` / `--ws-url` spellings.
- Newton skill is now vendored in-tree at `skill/newton/` (bumped to `1.2.1`); the root `skill-project.toml` `[dependencies.newton]` entry points at the local path. The standalone `gonewton/skill` repository is deprecated; its README will direct users to the canonical in-tree location.
- No behavioral or CLI changes: existing `monitor.conf` files using the previous split-port URLs continue to work; `--ailoop-http`, `--ailoop-ws`, and the two `monitor.conf` keys are unchanged.

### Optional MCP on the same URL as `newton serve` (issue #294)

- `newton serve` now accepts `--with-mcp` (opt-in, default off) and `--mcp-path <PATH>` (default `/mcp`) to mount the cli-framework MCP HTTP router on the same listener as the Newton REST API. One process, one port, one client URL prefix.
- When `--with-mcp` is absent the behavior of `newton serve` is unchanged (backward-compatible).
- Emits a single structured `mcp_serve_started` JSON log line on stderr (fields: `event`, `mcp_enabled`, `bind_address`, `mcp_path`, `tool_count`) when enabled.
- New error codes: `NEWTON-SERVE-MCP-001` (invalid `--mcp-path`), `NEWTON-SERVE-MCP-002` (path collides with existing REST route), `NEWTON-SERVE-MCP-003` (upstream mount API unavailable), `NEWTON-SERVE-MCP-004` (router construction failure).
- README and Newton skill updated with single-port topology docs and Cursor `mcpServers` HTTP example.

### GhOperator — transient-failure retry (issue #284)

- Engine retry loop now consults a per-error-code `is_retryable` classifier:
  malformed-JSON parse errors (`WFG-GH-002`), validation errors, auth errors,
  and most non-network gh failures fail fast instead of consuming retry
  attempts. Transient `WFG-GH-003` (spawn IO) and `WFG-GH-004` (non-zero exit
  with `TLS handshake timeout`/`dial tcp`/`i/o timeout`/`connection reset`/
  `EOF`/`HTTP 5xx` in stderr) remain retryable. Unknown codes default to
  retryable, preserving existing behavior for non-gh operators.
- New `MAX_TASK_BACKOFF_MS = 300_000` cap applied to per-attempt backoff (after
  multiplier, before sleep), mirroring `MAX_RETRY_DELAY_MS` in `gh.rs`.
- Per-retry `tracing::warn!` events now include `attempt`, `max_attempts`,
  `delay_ms`, `error_code`, and `operation` fields.
- `develop.yaml` template: `poll_pr` task now ships with a default
  `retry: { max_attempts: 5, backoff_ms: 2000, backoff_multiplier: 2.0,
  jitter_ms: 500 }` so transient `api.github.com` outages no longer abort
  develop runs.
- **Behavioral change** to `pr_create`: the operator-internal retry loop now
  uses exponential growth (`retry_multiplier`, default `2.0`) plus optional
  uniform jitter (`retry_jitter_ms`, default `0`). Previous behavior was a
  fixed-delay loop; pass `retry_multiplier: 1.0` to preserve byte-identical
  timing. The `ailoop` approver is still consulted at most once per logical
  invocation, regardless of retries.

### Breaking — CLI restructure (issue #273)

This is a clean cut: there are no aliases, no deprecation period, and no
migration shims. Scripts and dashboards must be updated in lockstep with the
binary.

**Top-level command renames**

| Was | Now |
|---|---|
| `newton validate <FILE>` | `newton workflow validate <FILE>` |
| `newton lint <FILE>` | `newton workflow lint <FILE>` |
| `newton explain <FILE>` | `newton workflow preview <FILE>` |
| `newton dot <FILE>` | `newton workflow graph <FILE>` (`--format dot`, `-o/--output`) |
| `newton log list` | `newton runs list` |
| `newton log show --execution-id <UUID>` | `newton runs show <RUN_ID>` |
| `newton checkpoints {list,clean}` | `newton checkpoint {list,clean}` |
| `newton artifacts clean` | `newton artifact clean` |

`newton run`, `resume`, `init`, `batch`, `serve`, `monitor`, and `webhook` keep
their top-level spellings; their flags changed (see below).

**Argument-shape changes**

- `newton run` now requires the workflow path as the sole positional
  argument; the legacy named alternative was removed.
- `workflow validate|lint|preview|graph` all take a required positional
  `<WORKFLOW>` and reject the legacy named flag.
- `newton runs show` takes `<RUN_ID>` positionally instead of via a named
  flag.
- `newton webhook serve` and `newton webhook status` now use a named
  workflow flag (`--workflow <PATH>`) and no longer accept a positional
  workflow argument.

**Flag renames**

| Was | Now |
|---|---|
| `--arg KEY=VAL` (run/preview) | `--trigger KEY=VAL` |
| `--set KEY=VAL` (run/preview) | `--context KEY=VAL` |
| `--trigger-json PATH` | `--trigger-file PATH` |
| `--max-time-seconds N` | `--timeout SECONDS` |
| `--out PATH` (graph) | `--output PATH` (`-o`) |
| `--execution-id UUID` (resume) | `--run-id UUID` |
| `--ui-dir PATH` (serve) | `--static-ui PATH` |
| `--http-url URL` (monitor) | `--ailoop-http URL` |
| `--ws-url URL` (monitor) | `--ailoop-ws URL` |
| `--backend` (monitor) | `--with-api` |
| `--sleep SECONDS` (batch) | `--poll-interval SECONDS` |
| `--template-source SOURCE` (init) | `--template SOURCE` |
| `--format-json` (checkpoint list) | `--json` |
| `--file PATH` (webhook serve/status) | `--workflow PATH` |

`--parallel-limit` is intentionally retained to stay aligned with the YAML
`parallel_limit` key.

**Telemetry**

- `LogInvocationKind` adds `Workflow`, `Runs`, `Checkpoint`, `Artifact`.
- `LogInvocationKind::Validate`, `Dot`, `Lint`, `Explain`, `Log`,
  `Checkpoints`, and `Artifacts` are removed (no compatibility shim).

**Not in this release (explicit):** `--allow-workflow-change` semantics fix
(separate PR), additional `--format` values for `workflow graph`, unit
suffixes on `--timeout`, and the YAML `parallel_limit` rename.

### Added

- **MCP server mode** (issue #237): top-level `--mcp-serve`, `--mcp-host`,
  `--mcp-port`, `--mcp-path` flags expose every registered Newton command
  (`REGISTERED_COMMAND_IDS`) as an MCP tool via the upstream `cli-framework`
  `mcp-server` feature. Newton operator defaults: `127.0.0.1:8730/mcp`
  (distinct from `newton serve`'s `8080`). Successful bind emits a single
  structured `event="mcp_serve_started"` log line carrying `mcp_enabled`,
  `bind_address`, `mcp_path`, and `tool_count`.
- New stable error codes:
  - `NEWTON-MCP-001` — TCP bind failure on the MCP listener; exits non-zero.
  - `NEWTON-MCP-002` — non-recoverable cli-framework MCP runtime error
    surfaced after a successful bind.
- `newton health`, `newton doctor`, `newton config show`, `newton completion`
  operational commands (issue #231).
- Feature-gated `newton ask "<query>"` substring router (`--features ask`)
  that ranks registered commands by `summary`/`syntax`/`category`.
- `LogInvocationKind::Diagnostic` for operational/diagnostic commands.

### Changed

- `cli-framework` git dependency now opts into `features = ["mcp-server"]`.
  Existing `newton serve` HTTP behaviour is unchanged. Note: upstream clap
  currently advertises `--mcp-port [default: 8080]` in `--help`; Newton's
  argv layer rewrites unset values to `8730` before dispatch. Operators
  should pass `--mcp-port` explicitly until the upstream default is aligned
  (tracked at cli-framework#29).

### Removed

- Legacy clap `Args`/`Command` enum and `pub async fn run` dispatcher in
  `crates/cli/src/cli/mod.rs`. Help text and argv parsing are now sourced
  exclusively from the cli-framework registry.
- `infer_log_invocation` argv scanner in `crates/cli/src/main.rs`,
  replaced by `cli/log_invocation.rs::kind_for_command`.

### Breaking changes

- **HIL operators now require ailoop unconditionally.** `HumanDecisionOperator` and
  `HumanApprovalOperator` no longer fall back to a console (stdin/stdout)
  interviewer when ailoop is not configured. A workflow containing a `human_decision`
  or `human_approval` task that runs without an enabled `AiloopContext` now fails
  at the first human task tick with error code `HIL-AILOOP-001` (category
  `ValidationError`). Workflows with no human task continue to run unchanged
  (the interviewer is constructed lazily on first prompt).
- **Legacy HIL transport override removed.** The previous environment variable
  that forced a console interviewer is no longer parsed, logged, or honoured.
  Ailoop owns transport selection (direct vs server mode) via its own
  `AILOOP_SERVER` / `--server` configuration.
- **`build_interviewer` removed.** Replaced by `resolve_interviewer` (eager,
  returns `Result<Arc<dyn Interviewer>, AppError>`) and `lazy_interviewer_provider`
  (deferred resolution for `BuiltinOperatorDeps`). External callers must migrate.
- **`BuiltinOperatorDeps.interviewer` field type changed** from
  `Option<Arc<dyn Interviewer>>` to `Option<InterviewerProvider>`, where
  `InterviewerProvider = Arc<dyn Fn() -> Result<Arc<dyn Interviewer>, AppError> + Send + Sync>`.
  The previous default that constructed a `ConsoleInterviewer` when the field was
  `None` has been removed.
- **`NEWTON_AILOOP_INTEGRATION=0|false|disabled` no longer suppresses HIL.** The
  flag continues to gate non-HIL ailoop features (events, notifier, output
  forwarder), but HIL workflows now require ailoop unconditionally.

### Upgrade path

For any deployment running workflows with `human_decision` / `human_approval`
tasks:

1. Install ailoop and ensure it is reachable (locally via direct mode, or
   remotely via a shared ailoop server).
2. Configure `.newton/configs/monitor.conf` with `ailoop_server_ws_url` and
   `ailoop_channel` pointing at your ailoop endpoint.
3. Set `NEWTON_AILOOP_INTEGRATION=1` in the Newton process environment.
4. Re-run the workflow. Misconfiguration now surfaces deterministically as
   `HIL-AILOOP-001` with a remediation pointer to
   `docs/operators/human_decision.md#configuration`.

### Added

- Failed workflow tasks now print a concise per-task diagnosis block to stderr
  (task id, error code/message, `exit_code`, and tail-truncated `stderr`/`stdout`
  capped at 16 KiB; `AgentOperator` artifact paths instead of stream bodies),
  in addition to the existing one-line stdout hint.
- New error codes `HIL-AILOOP-001` (no enabled ailoop context) and
  `HIL-AILOOP-003` (ailoop config load/parse failure, category `IoError`).
- `MockAiloopInterviewer` test double under
  `#[cfg(any(test, feature = "test-utils"))]` for use in HIL integration tests.
- `test-utils` Cargo feature on `crates/core` that exposes `MockAiloopInterviewer`
  outside `cfg(test)`.
- `require_enabled_ailoop_context` helper in
  `crates/core/src/integrations/ailoop/config.rs`.
