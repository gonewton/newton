# Tasks: Rust Newton Loop Implementation

**Input**: Design documents from `/specs/002-rust-newton-code/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

**Tests**: Test-first development required by constitution. Tests must be written before implementation and fail initially.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

- **Single project**: `src/`, `tests/` at repository root
- Paths assume the structure defined in plan.md

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and basic structure

- [x] T001 Create Cargo.toml with dependencies per research.md
- [x] T002 Create rust-toolchain.toml for nightly toolchain
- [x] T003 [P] Initialize src/main.rs binary entry point
- [x] T004 [P] Create src/lib.rs library interface
- [x] T005 [P] Setup directory structure per plan.md (src/cli/, src/core/, etc.)
- [x] T006 Configure cargo workspace and member crates if needed
- [x] T007 [P] Setup basic test structure (tests/unit/, tests/integration/, tests/cli/)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure that MUST be complete before ANY user story can be implemented

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [x] T008 [P] Implement core entities in src/core/entities.rs (OptimizationExecution, Iteration, Workspace, ErrorRecord, ToolResult)
- [x] T009 [P] Define enums and types in src/core/types.rs (ExecutionStatus, ErrorCategory, etc.)
- [x] T010 [P] Implement error handling infrastructure in src/core/error.rs (thiserror/anyhow integration)
- [x] T011 [P] Create workspace management in src/core/workspace.rs (validation, path handling)
- [x] T012 [P] Setup tracing/logging infrastructure in src/core/mod.rs
- [x] T013 [P] Implement serialization utilities in src/utils/serialization.rs (serde integration)
- [x] T014 [P] Create environment variable utilities in src/utils/env.rs (NEWTON_* variable handling)
- [x] T015 [P] Setup file I/O utilities in src/utils/files.rs (artifact management)

**Checkpoint**: Foundation ready - user story implementation can now begin in parallel

---

## Phase 3: User Story 1 - Complete Rust Port of Newton Loop Framework (Priority: P1) 🎯 MVP

**Goal**: Complete Rust implementation of Newton Loop with identical functionality to Python version

**Independent Test**: Run workspace examples and verify identical behavior/output to Python version

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T016 [P] [US1] CLI snapshot tests for run command in tests/cli/snapshots/
- [ ] T017 [P] [US1] CLI snapshot tests for status command in tests/cli/snapshots/
- [ ] T018 [P] [US1] CLI snapshot tests for report command in tests/cli/snapshots/
- [ ] T019 [P] [US1] Integration test for full optimization workflow in tests/integration/test_workflow.rs
- [ ] T020 [P] [US1] Unit tests for core entities in tests/unit/test_entities.rs

### Implementation for User Story 1

- [ ] T021 [US1] Implement OptimizationOrchestrator in src/core/orchestrator.rs (depends on T008-T015)
- [ ] T022 [US1] Create history recorder in src/core/history_recorder.rs (execution tracking)
- [ ] T023 [US1] Implement results processor in src/core/results_processor.rs (report generation)
- [ ] T024 [US1] Create CLI argument parsing in src/cli/args.rs (clap integration)
- [ ] T025 [US1] Implement CLI command handlers in src/cli/commands.rs (run, step, status, report)
- [ ] T026 [US1] Setup main CLI entry point in src/main.rs (command routing)
- [ ] T027 [US1] Add comprehensive logging throughout application (tracing integration)
- [ ] T028 [US1] Implement workspace validation and initialization
- [ ] T029 [US1] Add resource limit enforcement (iterations, time, tool timeouts)
- [ ] T030 [US1] Create artifact file management system

**Checkpoint**: At this point, User Story 1 should be fully functional and testable independently

---

## Phase 4: User Story 2 - Strict Toolchain Mode Compatibility (Priority: P1)

**Goal**: Support strict toolchain mode with CLI tool commands, timeouts, and environment variables

**Independent Test**: Use existing workspace examples with Rust binary and verify tool execution with correct environment variables and timeouts

### Tests for User Story 2 ⚠️

- [ ] T031 [P] [US2] Tool execution contract tests in tests/contract/test_tool_execution.rs
- [ ] T032 [P] [US2] Environment variable integration tests in tests/integration/test_environment.rs
- [ ] T033 [P] [US2] Timeout handling tests in tests/integration/test_timeouts.rs

### Implementation for User Story 2

- [ ] T034 [US2] Implement StrictToolchainRunner in src/tools/strict_toolchain.rs (depends on T021)
- [ ] T035 [US2] Create tool execution interfaces in src/tools/mod.rs (evaluator, advisor, executor traits)
- [ ] T036 [US2] Implement subprocess execution with tokio::process in src/tools/execution.rs
- [ ] T037 [US2] Add timeout handling for tool execution (global and per-tool timeouts)
- [ ] T038 [US2] Implement environment variable setting (NEWTON_* variables per contracts/)
- [ ] T039 [US2] Create artifact directory management for iteration-specific files
- [ ] T040 [US2] Add tool result capture and validation (exit codes, output handling)
- [ ] T041 [US2] Integrate strict toolchain mode with orchestrator (US1 integration)

**Checkpoint**: At this point, User Stories 1 AND 2 should both work independently

---

## Phase 5: User Story 3 - Command Line Interface Compatibility (Priority: P1)

**Goal**: Provide identical CLI interface with all commands, options, and output formatting

**Independent Test**: Run all CLI commands with various options and compare output format/behavior to Python version

### Tests for User Story 3 ⚠️

- [ ] T042 [P] [US3] CLI argument parsing tests in tests/unit/test_cli_args.rs
- [ ] T043 [P] [US3] Command handler integration tests in tests/integration/test_commands.rs
- [ ] T044 [P] [US3] Output formatting snapshot tests in tests/cli/test_output_formatting.rs

### Implementation for User Story 3

- [ ] T045 [US3] Implement complete CLI argument definitions in src/cli/args.rs (all options from contracts/cli-api.yaml)
- [ ] T046 [US3] Add help text and usage messages (match Python version exactly)
- [ ] T047 [US3] Implement run command handler with all options (--max-iterations, --max-time, etc.)
- [ ] T048 [US3] Implement step command handler (single iteration execution)
- [ ] T049 [US3] Implement status command handler (execution monitoring)
- [ ] T050 [US3] Implement report command handler (results generation with JSON/text output)
- [ ] T051 [US3] Add error output formatting (match Python stderr format)
- [ ] T052 [US3] Implement exit code handling (match Python version exactly)
- [ ] T053 [US3] Add progress indicators and status messages (optional TTY-aware)

**Checkpoint**: All P1 user stories should now be independently functional

---

## Phase 6: User Story 4 - Error Handling and Reporting Compatibility (Priority: P2)

**Goal**: Provide identical error handling, categorization, and reporting features

**Independent Test**: Trigger various error conditions and compare error reports, categories, and recovery suggestions to Python version

### Tests for User Story 4 ⚠️

- [ ] T054 [P] [US4] Error categorization tests in tests/unit/test_error_categorization.rs
- [ ] T055 [P] [US4] Error reporting integration tests in tests/integration/test_error_reporting.rs
- [ ] T056 [P] [US4] Recovery suggestion tests in tests/unit/test_recovery_suggestions.rs

### Implementation for User Story 4

- [ ] T057 [US4] Implement ErrorHandler in src/core/error_handler.rs (categorization, severity, context)
- [ ] T058 [US4] Add error reporting functionality (error ID generation, structured reports)
- [ ] T059 [US4] Implement recovery suggestions for different error types
- [ ] T060 [US4] Add error context collection (execution ID, iteration number, component info)
- [ ] T061 [US4] Create error persistence and retrieval system
- [ ] T062 [US4] Integrate error handling throughout application (CLI commands, tool execution)
- [ ] T063 [US4] Add error logging and tracing integration

**Checkpoint**: User Stories 1-4 should all work independently with comprehensive error handling

---

## Phase 7: User Story 5 - Performance Characteristics (Priority: P3)

**Goal**: Provide better performance while maintaining identical behavior

**Independent Test**: Run identical workloads and compare execution times, memory usage, and resource consumption

### Tests for User Story 5 ⚠️

- [ ] T064 [P] [US5] Performance benchmark tests in benches/toolchain_benchmark.rs
- [ ] T065 [P] [US5] Memory usage tests in tests/integration/test_memory_usage.rs
- [ ] T066 [P] [US5] Execution time comparison tests in tests/integration/test_execution_time.rs

### Implementation for User Story 5

- [ ] T067 [US5] Optimize tool execution performance (subprocess overhead reduction)
- [ ] T068 [US5] Implement efficient file I/O for artifact management
- [ ] T069 [US5] Add memory-efficient data structures for large workspaces
- [ ] T070 [US5] Optimize serialization/deserialization performance
- [ ] T071 [US5] Add resource monitoring and performance metrics
- [ ] T072 [US5] Implement caching for frequently accessed data (if beneficial)
- [ ] T073 [US5] Profile and optimize hot paths in orchestration logic

**Checkpoint**: All user stories should now be independently functional with performance optimizations

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Improvements that affect multiple user stories

- [ ] T074 [P] Add comprehensive documentation (README.md, API docs)
- [ ] T075 [P] Implement code coverage reporting (cargo llvm-cov)
- [ ] T076 [P] Add performance benchmarks for regression testing
- [ ] T077 [P] Create example workspaces for testing (budgeting, healthy grocery)
- [ ] T078 [P] Add integration tests with real Python Newton Loop examples
- [ ] T079 [P] Implement configuration file support (optional, backwards compatible)
- [ ] T080 [P] Add comprehensive logging configuration options
- [ ] T081 [P] Create packaging and distribution setup (cargo package, release workflow)
- [ ] T082 [P] Add CI/CD pipeline configuration (GitHub Actions)
- [ ] T083 Validate quickstart.md instructions work end-to-end
- [ ] T084 Run final compatibility testing against Python version
- [ ] T085 Performance regression testing and optimization

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies - can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion - BLOCKS all user stories
- **User Stories (Phase 3-7)**: All depend on Foundational phase completion
  - User stories can then proceed in parallel (if staffed)
  - Or sequentially in priority order (P1 → P2 → P3)
- **Polish (Phase 8)**: Depends on all desired user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: Can start after Foundational - Foundation for all other stories
- **User Story 2 (P1)**: Can start after US1 - Builds on orchestration framework
- **User Story 3 (P1)**: Can start after Foundational - Independent CLI implementation
- **User Story 4 (P2)**: Can start after Foundational - Independent error handling
- **User Story 5 (P3)**: Can start after US1-US4 - Performance optimization of working system

### Within Each User Story

- Tests (if included) MUST be written and FAIL before implementation
- Core entities before orchestration
- Tool execution before CLI commands
- Error handling integrated throughout
- Story complete before moving to next priority

### Parallel Opportunities

- All Setup tasks marked [P] can run in parallel
- All Foundational tasks marked [P] can run in parallel (within Phase 2)
- Once Foundational phase completes, all user stories can start in parallel (if team capacity allows)
- All tests for a user story marked [P] can run in parallel
- Different user stories can be worked on in parallel by different team members

---

## Parallel Example: User Story 1

```bash
# Launch all tests for User Story 1 together:
cargo nextest run --package cli_tests -- test_run_command
cargo nextest run --package cli_tests -- test_status_command
cargo nextest run --package integration_tests -- test_full_workflow

# Launch all core components for User Story 1 together:
cargo build --bin newton  # CLI
cargo test --package core --lib  # Core library tests
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL - blocks all stories)
3. Complete Phase 3: User Story 1
4. **STOP and VALIDATE**: Test User Story 1 independently
5. Deploy/demo if ready

### Incremental Delivery

1. Complete Setup + Foundational → Foundation ready
2. Add User Story 1 → Test independently → Deploy/Demo (MVP!)
3. Add User Story 2 → Test independently → Deploy/Demo
4. Add User Story 3 → Test independently → Deploy/Demo
5. Add User Story 4 → Test independently → Deploy/Demo
6. Add User Story 5 → Test independently → Deploy/Demo
7. Each story adds value without breaking previous stories

### Parallel Team Strategy

With multiple developers:

1. Team completes Setup + Foundational together
2. Once Foundational is done:
   - Developer A: User Story 1 (Core Framework)
   - Developer B: User Story 2 (Toolchain Mode)
   - Developer C: User Story 3 (CLI Interface)
   - Developer D: User Story 4 (Error Handling)
   - Developer E: User Story 5 (Performance)
3. Stories complete and integrate independently

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- Each user story should be independently completable and testable
- Verify tests fail before implementing
- Commit after each task or logical group
- Stop at any checkpoint to validate story independently
- Avoid: vague tasks, same file conflicts, cross-story dependencies that break independence
- Constitution requires test-first development and Rust language excellence
- All tasks must maintain 100% compatibility with Python Newton Loop