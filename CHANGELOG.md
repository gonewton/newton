# Changelog

All notable changes to this project will be documented in this file.

## Unreleased

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
