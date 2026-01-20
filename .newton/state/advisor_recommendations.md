**Plan of Actions:**

1. **Resolve Rust version conflict** - Either:
   - Upgrade Rust compiler to version 1.82 or newer (preferred)
   - OR downgrade `indexmap v2.13.0` to a compatible version for rustc 1.77.0-nightly

2. **Verify project builds successfully** after resolving version conflict

3. **Implement goal: Get the current year**
   - Determine where to implement the current year logic in the codebase
   - Write code to retrieve and return the current year
   - Run tests to verify functionality
   - Commit the changes when working
