# Research Findings: Rust Newton Loop Implementation

**Date**: 2026-01-27 (Updated)
**Feature**: 002-rust-newton-code
**Research Focus**: Implementation approaches for 100% compatible Rust port of Newton Loop

## Decision: Rust Version and Toolchain

**Chosen**: Rust 1.93.0 Stable (released January 22, 2026)
**Rationale**: Latest stable version with all bug fixes and optimizations for CLI applications.
**Alternatives Considered**:
- Rust 1.92.x: Previous stable, but lacks latest improvements
- Rust nightly: Not recommended for production CLI tool due to instability

## Decision: Async Runtime Selection

**Chosen**: tokio 1.49 with single-threaded execution
**Rationale**: Newton Loop executes tools sequentially without concurrency within a single optimization run. Tokio provides excellent subprocess handling and async I/O, while single-threaded execution ensures deterministic behavior matching Python version.
**Alternatives Considered**:
- async-std: More minimal but less ecosystem maturity than tokio
- std::thread: Would require manual coordination, more complex than needed for sequential execution

## Decision: Error Handling Architecture

**Chosen**: thiserror for structured errors, anyhow for propagation
**Rationale**: Matches constitution requirements and provides excellent ergonomics. thiserror enables type-safe error enums, while anyhow allows context-rich error propagation without changing error types.
**Alternatives Considered**:
- Custom error types only: Would require more boilerplate and less flexible context addition
- Box<dyn Error>: Less type-safe and harder to handle specific error cases

## Decision: CLI Argument Parsing

**Chosen**: clap 4.5 with derive API
**Rationale**: Provides type-safe argument parsing with excellent error messages and help generation. Derive API reduces boilerplate while maintaining flexibility for complex CLI structures.
**Alternatives Considered**:
- clap builder API: More verbose but allows runtime configuration
- structopt: Deprecated in favor of clap derive

## Decision: Logging Framework

**Chosen**: tracing with custom subscriber
**Rationale**: Constitution specifies tracing. Provides structured logging with excellent performance and ecosystem integration. Allows runtime configuration via RUST_LOG.
**Alternatives Considered**:
- log crate: Less feature-rich than tracing
- env_logger: Sufficient but less flexible than tracing ecosystem

## Decision: Configuration Management

**Chosen**: Environment variables only (matching Python version)
**Rationale**: Newton Loop uses environment variables exclusively for tool communication. No configuration files needed - keeps interface simple and compatible.
**Alternatives Considered**:
- TOML config files: Would add complexity not present in Python version
- clap defaults: Less flexible for environment-based configuration

## Decision: Template Processing

**Chosen**: Custom envsubst-like implementation
**Rationale**: Must match Python's environment variable substitution behavior. Simple, deterministic processing without external dependencies for core functionality.
**Alternatives Considered**:
- tera/jinja2-like engines: Overkill for simple variable substitution
- regex replacement: Less reliable for complex variable patterns

## Decision: File I/O Strategy

**Chosen**: Standard library with error handling
**Rationale**: Simple file operations for workspace and artifact management. Standard library provides all needed functionality with proper error handling.
**Alternatives Considered**:
- tokio::fs: Unnecessary for non-async file operations
- tempfile: Not needed for persistent artifact storage

## Decision: Agent Abstraction Design

**Chosen**: Trait-based abstraction with enum dispatch
**Rationale**: Allows compile-time safety for agent types while supporting extensibility. Easy to test with mock implementations.
**Alternatives Considered**:
- Dynamic dispatch: Runtime overhead with less type safety
- Builder pattern: More complex than needed for three agent types

## Decision: Process Execution

**Chosen**: tokio::process with timeout handling
**Rationale**: Excellent subprocess management with async timeout support. Matches the sequential, timeout-constrained execution model of Newton Loop.
**Alternatives Considered**:
- std::process: Blocking execution doesn't work well with timeouts
- Command execution crates: tokio::process is sufficient and well-integrated

## Decision: Serialization Format

**Chosen**: serde_json for JSON, custom Markdown parsing
**Rationale**: JSON for structured data (reports, status), custom Markdown parsing for Newton Loop artifact compatibility. Serde provides excellent performance and ergonomics.
**Alternatives Considered**:
- Custom JSON implementation: More error-prone than serde
- TOML/YAML: Not used by Newton Loop framework

## Decision: Module Organization

**Chosen**: Constitution-aligned structure
**Rationale**: Follows constitution guidelines: cli/ (parsing), core/ (orchestrator, agent, templates), tools/ (evaluator, advisor, executor), utils/ (env, files).
**Alternatives Considered**:
- Flat structure: Violates constitution modularity requirements
- Domain-driven: Overkill for CLI tool scope

## Decision: Testing Framework

**Chosen**: cargo nextest + insta + assert_cmd
**Rationale**: cargo nextest provides 3x faster test execution with better parallelization. insta enables snapshot testing for CLI output validation. assert_cmd specializes in CLI testing.
**Alternatives Considered**:
- Standard cargo test: Slower and less feature-rich
- Custom test frameworks: Unnecessary complexity

## Decision: Performance Targets

**Chosen**: Specific performance benchmarks
**Rationale**: Based on research for external process execution in CLI tools.
**Targets**:
- Process spawn overhead: < 10ms cold, < 5ms warm
- Throughput: 100-500 processes/second
- Memory usage: Monitor for process leaks
- Parallelization: 2-4x CPU cores max

## Decision: Dependency Versions

**Chosen**: Latest stable versions as of January 2026
```toml
[dependencies]
clap = { version = "4.5", features = ["derive"] }
tokio = { version = "1.49", features = ["full"] }
anyhow = "1.0"
thiserror = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1.0", features = ["v4", "serde"] }
```
**Rationale**: These versions represent the latest stable releases with proven stability.

## Decision: Compatibility Testing

**Chosen**: Snapshot-based compatibility testing
**Rationale**: Ensures 100% output compatibility with Python version through automated comparison using insta snapshots.
**Alternatives Considered**:
- Manual testing: Too error-prone and time-consuming
- Property-based testing: Not suitable for exact compatibility validation