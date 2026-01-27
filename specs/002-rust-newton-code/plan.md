# Implementation Plan: Rust Newton Loop Implementation

**Branch**: `002-rust-newton-code` | **Date**: 2026-01-27 | **Spec**: `/specs/002-rust-newton-code/spec.md`
**Input**: Feature specification from `/specs/002-rust-newton-code/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command. See `.specify/templates/commands/plan.md` for the execution workflow.

## Summary

Create a complete Rust implementation of the Newton Loop optimization framework that provides 100% compatibility with the existing Python version. The implementation must maintain identical CLI interface, behavior, and output while leveraging Rust's performance and reliability benefits.

## Technical Context

**Language/Version**: Rust 1.93.0 Stable  
**Primary Dependencies**: clap 4.5 (CLI parsing), tokio 1.49 (async runtime), anyhow/thiserror 1.0 (error handling), tracing 0.1 (logging), serde 1.0.228/serde_json 1.0 (serialization), chrono 0.4 (time), uuid 1.0 (execution IDs)  
**Storage**: Files (artifact management, workspace state)  
**Testing**: cargo nextest 0.11, insta 1.0 (snapshot testing), assert_cmd 2.0 (CLI testing)  
**Target Platform**: Linux (primary), cross-platform compatibility for development
**Project Type**: Single project CLI tool  
**Performance Goals**: Identical behavior to Python version with better resource efficiency; process spawn <10ms cold/<5ms warm, 100-500 processes/second throughput  
**Constraints**: 100% CLI compatibility with Python Newton Loop, MUST pass all compatibility gates  
**Scale/Scope**: Workspace sizes up to 1GB, artifact directories up to 100MB per iteration, maximum 1000 iterations per execution

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

### I. Test-First Development
- [x] Test strategy defined (unit, integration, CLI, snapshot tests - from research.md)
- [x] Test coverage target: 80%+ for new code (from constitution)
- [x] Tests can be written before implementation (cargo nextest + insta setup)

### II. Rust Language Excellence
- [x] Uses Rust 1.93.0 Stable toolchain (from research.md)
- [x] Follows Rust Style Guide (cargo fmt, cargo clippy -D warnings - from research.md)
- [x] Uses thiserror/anyhow for error handling (from research.md)
- [x] Appropriate use of pub(crate) vs pub (from research.md structure)

### III. CLI-First Interface
- [x] All functionality accessible via CLI (clap 4.5 from research.md)
- [x] Compatible with Newton Loop framework interface (contracts/cli-api.yaml)
- [x] Supports both JSON and human-readable output (contracts/cli-api.yaml)

### IV. Newton Loop Integration
- [x] Environment variables match Newton Loop expectations (contracts/environment-api.yaml)
- [x] Artifact file formats match Markdown structure (contracts/environment-api.yaml)
- [x] No modifications to Newton Loop framework required (design complete)

### V. Agent Abstraction Layer
- [x] Supports extensible agent interface (data-model.md ToolResult entity)
- [x] Agent errors properly handled and logged (data-model.md ErrorRecord entity)
- [x] Agent abstraction independently testable (contract tests defined)

### VI. Template System
- [x] Template variable substitution implemented (environment variables from contracts)
- [x] Template loading handles missing files gracefully (workspace validation in data-model.md)
- [x] Template processing is deterministic and testable (defined in quickstart.md)

## Project Structure

### Documentation (this feature)

```text
specs/002-rust-newton-code/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (/speckit.plan command)
└── tasks.md             # Phase 2 output (/speckit.tasks command - already created)
```

### Source Code (repository root)

```text
src/
├── cli/
│   ├── args.rs          # CLI argument parsing
│   ├── commands.rs      # CLI command handlers
│   └── mod.rs
├── core/
│   ├── entities.rs      # OptimizationExecution, Iteration, Workspace, ErrorRecord, ToolResult
│   ├── orchestrator.rs  # OptimizationOrchestrator
│   ├── history_recorder.rs
│   ├── results_processor.rs
│   ├── error_handler.rs
│   ├── types.rs         # ExecutionStatus, ErrorCategory, enums
│   ├── error.rs         # Error handling infrastructure
│   ├── workspace.rs     # Workspace management
│   └── mod.rs
├── tools/
│   ├── strict_toolchain.rs  # StrictToolchainRunner
│   ├── execution.rs         # Subprocess execution
│   └── mod.rs
└── utils/
    ├── serialization.rs     # Serde integration
    ├── env.rs               # Environment variable utilities
    ├── files.rs             # File I/O utilities
    └── mod.rs

tests/
├── unit/                    # Unit tests for core modules
├── integration/             # Integration tests for workflows
├── cli/                     # CLI-specific tests and snapshots
└── contract/                # Contract tests for external interfaces

Cargo.toml                  # Project dependencies
rust-toolchain.toml         # Rust toolchain specification
```

**Structure Decision**: Single project structure aligned with tasks.md implementation plan. Framework-focused organization separating CLI interface, core engine, tool execution, and utilities.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| None | All constitution requirements met through standard Rust practices | N/A |

## Phase Completion Summary

### Phase 0: Research ✅ COMPLETE
- **Resolved**: All NEEDS CLARIFICATION items in technical context
- **Deliverables**: `research.md` with specific version numbers and implementation decisions
- **Decisions Made**: Rust 1.93.0, tokio 1.49, clap 4.5, testing with cargo nextest + insta

### Phase 1: Design ✅ COMPLETE  
- **Deliverables**: `data-model.md`, `contracts/`, `quickstart.md`
- **Data Model**: Complete entity definitions with Rust types and validation rules
- **Contracts**: CLI API and environment variable specifications
- **Quickstart**: Comprehensive usage and development guide

### Phase 2: Planning ✅ READY
- **Tasks Available**: `tasks.md` already exists with comprehensive task breakdown
- **Implementation Ready**: All prerequisites met for `/speckit.implement`

## Next Steps

All planning phases are complete. The implementation is ready to begin with:

1. **Foundation Phase** (T001-T015): Project setup and core infrastructure
2. **User Story Implementation** (T016+): Sequential implementation per tasks.md
3. **Quality Gates**: Constitution compliance verified throughout

**Status**: ✅ READY FOR IMPLEMENTATION
