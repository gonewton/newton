<!--
Sync Impact Report - Constitution v1.2.0 (2026-01-19)
Version change: 1.1.0 → 1.2.0 (MINOR: Removed specific scale requirements for broader compatibility)
Modified principles: Simplified performance and resource requirements to focus on behavioral compatibility
Added sections: None
Removed sections: Specific workspace size, threading, and timeout constraints
Templates requiring updates: ✅ plan-template.md (removed scale constraints), ✅ spec-template.md (removed scale requirements)
Follow-up TODOs: None
-->
# Newton Loop Framework Constitution (Rust Implementation)

## Core Principles

### I. 100% Compatibility with Python Newton Loop (NON-NEGOTIABLE)
The Rust implementation MUST provide identical functionality, behavior, and output as the Python Newton Loop framework. All CLI commands, options, output formatting, error messages, and exit codes must match exactly. Existing workspaces and toolchains must work without modification. Success criteria: All existing Newton Loop examples execute successfully with identical optimization results within 5% variance.

### II. Strict Toolchain Mode Implementation (NON-NEGOTIABLE)
System MUST support strict toolchain mode with explicit CLI tool commands (--evaluator-cmd, --advisor-cmd, --executor-cmd) and all associated options. Tools must receive identical NEWTON_* environment variables as the Python version. Tool timeouts (--tool-timeout-seconds, --evaluator-timeout, etc.) must be handled identically with proper error reporting and termination behavior.

### III. Complete CLI Interface Compatibility (NON-NEGOTIABLE)
System MUST implement all CLI commands (run, step, status, report) with identical arguments, options, help text, and output formatting. Command-line parsing must accept/reject the same inputs with identical error messages and exit codes. The --help output must be identical to the Python version for all commands.

### IV. Comprehensive Error Handling Compatibility (NON-NEGOTIABLE)
Error handling, categorization, severity assessment, and recovery suggestions MUST match the Python version exactly. System MUST implement identical error records, validation messages, and failure recovery behavior. Error reports and status information must have identical structure and content.

### V. Performance Excellence with Rust Benefits
While maintaining 100% behavioral compatibility, leverage Rust's performance advantages for better execution speed, memory efficiency, and stability. Resource usage should be more efficient than Python version while producing identical results.

### VI. Rust Language Excellence & Testing Standards
Maintain consistent high-quality Rust standards following official style guide. Use `cargo fmt`, `cargo clippy -D warnings`, and comprehensive testing. All code changes require tests first with >80% coverage. Required test types: unit tests, integration tests, CLI compatibility tests, and snapshot tests for output validation. Tests must validate against Python version behavior for compatibility assurance.

## Technology Stack

**Language**: Rust (stable toolchain, focus on reliability and performance)  
**Primary Dependencies**: `clap` (CLI parsing), `tokio` (async runtime), `anyhow`/`thiserror` (error handling), `tracing` (structured logging), `serde`/`serde_json` (data serialization), `chrono` (time handling), `uuid` (execution IDs)  
**Testing**: `cargo nextest` (test runner), `insta` (snapshot testing for CLI output validation), `assert_cmd` (CLI testing), `tempfile` (workspace testing)  
**External Integration**: CLI tool execution via subprocess with environment variable communication  
**Target Platform**: Linux (primary), cross-platform compatibility for development environments  
**Resource Limits**: Compatible with Python Newton Loop resource management patterns

## Development Workflow

### Compatibility Testing Gates
All PRs must pass:
1. `cargo fmt --all` (formatting)
2. `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` (linting)
3. `cargo nextest run` (all tests pass)
4. `cargo insta test --check` (CLI output snapshots validated)
5. **Compatibility validation**: Run against Python Newton Loop test suites and verify identical behavior

### Compatibility Assurance
- All CLI commands must produce identical output format and exit codes
- Environment variable communication must match Python version exactly
- Error messages and categorization must be identical
- Workspace validation rules must accept/reject same inputs
- Resource limit enforcement must behave identically

### Error Handling & Logging
Use `thiserror` for structured error types matching Python version categories. Use `anyhow::Context` for propagation. Implement identical error severity levels and recovery suggestions. Use `tracing` for structured logging with fields: execution_id, iteration, tool_type, workspace_path, timing information.

### Module Organization
Framework-focused structure: `cli/` (command parsing), `core/` (execution engine, iteration logic), `workspace/` (validation, artifact management), `tools/` (evaluator/advisor/executor execution), `models/` (data entities), `utils/` (file ops, environment). Use `pub(crate)` for internal APIs, `pub` only for framework entry points.

## Governance

This constitution establishes the development principles for the complete Rust implementation of Newton Loop framework. Amendments require:
1. Documentation of rationale and compatibility impact
2. Update to version number (semantic versioning: MAJOR for breaking compatibility, MINOR for feature additions, PATCH for clarifications)
3. Update to dependent templates (plan-template.md, spec-template.md, tasks-template.md)
4. Compatibility testing against Python version
5. Sync Impact Report documenting changes and compatibility assurance

All PRs and reviews must verify 100% compatibility with Python Newton Loop. Any deviation from Python behavior requires explicit justification and compatibility testing. Code complexity must be justified by performance or maintainability benefits. Follow STYLE.md and CONTRIBUTING.md guidelines for implementation details.

**Version**: 1.2.0 | **Ratified**: 2026-01-19 | **Last Amended**: 2026-01-19
