# Research Findings: Rust Newton Loop Implementation

**Date**: 2026-01-19
**Feature**: 002-rust-newton-code
**Research Focus**: Implementation approaches for 100% compatible Rust port of Newton Loop

## Decision: Async Runtime Selection

**Chosen**: tokio with single-threaded execution
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

**Chosen**: clap with derive API
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