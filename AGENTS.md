These are strict orders that you MUST follow:

R0001 - You must always uses conventional commit messages.

There are prohibited operations:

P0001 - You are prohibited to use --no-verify when commiting

## Project Guidelines

### Development Workflow
- Always run tests before pushing code changes
- Ensure code formatting with `cargo fmt` passes
- Run `cargo clippy` to catch potential issues
- Follow Rust best practices and idioms
- Write comprehensive tests for new features

### Commit Message Conventions
- Use conventional commit format: `type(scope): description`
- Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`
- Keep first line under 72 characters
- Use imperative mood in descriptions

### Code Quality Standards
- All public APIs must have documentation
- Error handling should be comprehensive and user-friendly
- Performance considerations should be documented
- Security implications must be evaluated for new features

### Testing Requirements
- Unit tests for all public functions
- Integration tests for complex workflows
- Contract tests for API boundaries
- Performance benchmarks for critical paths

### CI/CD Expectations
- All CI checks must pass before merging
- Security audits must pass without vulnerabilities
- Code coverage should be maintained or improved
- Release builds must compile successfully on all target platforms