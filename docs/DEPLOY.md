## Deployment Playbook

## Deployment Playbook

Use this checklist whenever you need to ship code all the way through the GitHub release pipeline and into Homebrew.

### 1. Prepare the change locally

1. Verify the tree is clean: `git status -sb`.
2. Implement the feature/fix and update docs/tests.
3. Run the required checks:
   - `cargo fmt`
   - `cargo clippy --all-targets --all-features`
   - `cargo test --all`
4. Commit with a conventional message, e.g. `feat(cli): add new flag`.

### 2. Push and monitor CI

1. Push to `main`: `git push`.
2. Find the CI run ID: `gh run list --limit 5` (look for workflow `CI`).
3. Watch it finish: `gh run watch <CI_RUN_ID> --exit-status`.
4. If it fails, fix locally and repeat.

### 3. Track the Auto Release bump

1. Auto Release triggers after CI. Identify it via `gh run list --limit 5` (workflow `Auto Release`).
2. Stream progress: `gh run watch <AUTO_RELEASE_RUN_ID> --exit-status`.
3. When it completes, pull the bot commit/tag: `git pull`.

### 4. Ensure the Release workflow runs

1. Auto Release pushes a tag (e.g. `v0.3.9`). If correction is needed, delete/re-push: `git push origin :refs/tags/vX.Y.Z && git push origin vX.Y.Z`.
2. Confirm the Release workflow fired: `gh run list --workflow Release --limit 3`.
3. Watch it end-to-end (builds, GitHub release, Homebrew/Scoop updates): `gh run watch <RELEASE_RUN_ID> --exit-status`.
4. Verify the release exists: `gh release view vX.Y.Z`.

### 5. Validate package-manager updates

1. Inspect the Homebrew formula to ensure URL/version/hash align:
   - `gh api repos/gonewton/homebrew-cli/contents/Formula/newton.rb?ref=main --jq '.content' | base64 -d`
2. (Optional) check the Scoop manifest similarly.

### 6. Verify via Homebrew locally

1. Refresh taps: `brew update`.
2. Install the new build: `brew upgrade newton` (or `brew install newton` if not present).
3. Sanity check the CLI:
   - `newton --version`
   - `newton --help` — the banner should show the new version number.

### 7. Troubleshooting tips

- Use `gh run view <RUN_ID> --log` to inspect failures in any workflow stage.
- If Auto Release bumps the version unexpectedly, pull before continuing to avoid conflicts.
- Release workflow will fail if the tag version mismatches `Cargo.toml`; re-tag after correcting.
