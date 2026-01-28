NEWTON-0001 - You MUST always use conventional commit messages
NEWTON-0002 - You MUST NOT use --no-verify when committing
NEWTON-0003 - You MUST run tests before pushing code changes
NEWTON-0004 - You MUST ensure code formatting with `cargo fmt` passes
NEWTON-0005 - You SHOULD run `cargo clippy` to catch potential issues
NEWTON-0006 - You SHOULD follow Rust best practices and idioms
NEWTON-0007 - You MUST write comprehensive tests for new features
NEWTON-0008 - You MUST use conventional commit format: `type(scope): description`
NEWTON-0009 - You MUST keep the first line under 72 characters
NEWTON-0010 - You MUST use imperative mood in descriptions
NEWTON-0011 - All public APIs MUST have documentation
NEWTON-0012 - Error handling SHOULD be comprehensive and user-friendly
NEWTON-0013 - Performance considerations SHOULD be documented
NEWTON-0014 - Security implications MUST be evaluated for new features
NEWTON-0015 - You MUST write unit tests for all public functions
NEWTON-0016 - You SHOULD write integration tests for complex workflows
NEWTON-0017 - You SHOULD write contract tests for API boundaries
NEWTON-0018 - You SHOULD write performance benchmarks for critical paths
NEWTON-0019 - All CI checks MUST pass before merging
NEWTON-0020 - Security audits MUST pass without vulnerabilities
NEWTON-0021 - Code coverage SHOULD be maintained or improved
NEWTON-0022 - Release builds MUST compile successfully on all target platforms
NEWTON-0023 - You MUST assume git is installed on the user machine and MUST NOT check for git presence
NEWTON-0024 - You SHOULD NOT adopt an extremely defensive coding style with excessive validation

## Active Technologies
- Rust 1.93.0 Stable + clap 4.5 (CLI parsing), tokio 1.49 (async runtime), anyhow/thiserror 1.0 (error handling), tracing 0.1 (logging), serde 1.0.228/serde_json 1.0 (serialization), chrono 0.4 (time), uuid 1.0 (execution IDs) (002-rust-newton-code)
- Files (artifact management, workspace state) (002-rust-newton-code)

## Recent Changes
- 002-rust-newton-code: Added Rust 1.93.0 Stable + clap 4.5 (CLI parsing), tokio 1.49 (async runtime), anyhow/thiserror 1.0 (error handling), tracing 0.1 (logging), serde 1.0.228/serde_json 1.0 (serialization), chrono 0.4 (time), uuid 1.0 (execution IDs)
