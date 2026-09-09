# Deployment Playbook

This document describes the CI/CD process for newton: validations, stages, and how to ship code through the pipeline to GitHub releases and package managers (Homebrew, Scoop).

**Quick overview.** Pushes and pull requests to `main` run **CI** (format, clippy, build, tests, security audit). After a code merge to `main`, **Auto Release** creates a patch-version PR and enables squash auto-merge. Branch protection holds that PR until its required checks pass. When the release PR lands, Auto Release tags that exact commit and the **Release** workflow publishes Linux and Windows binaries, then updates Homebrew and Scoop. Documentation-only changes and commits containing `[skip release]` do not create a release PR.

## CI/CD Overview

```mermaid
flowchart TB
  subgraph dev["Development"]
    A[Branch / PR to main]
    B[CI workflow]
  end

  subgraph ci["CI (ci.yml)"]
    B --> C[Format check]
    C --> D[Clippy]
    D --> E[Build release]
    E --> F[Run tests]
    F --> G[Upload binary artifact]
    B --> H[Security audit]
  end

  subgraph gate["Merge gate"]
    I{Merge to main}
    I --> J{Paths changed?}
    J -->|docs, .github, README, CHANGELOG only| K[No Auto Release]
    J -->|Code / Cargo.toml etc.| L{Skip flags?}
    L -->|skip release or release commit| K
    L -->|No skip| M[Auto Release]
  end

  subgraph auto["Auto Release (auto-release.yml)"]
    M --> P[Create release branch]
    P --> Q[Bump patch in Cargo.toml]
    Q --> R[Commit & push branch]
    R --> S[Create/update release PR]
    S --> T[Enable auto-merge]
    T --> N[Required checks pass]
    N --> O[Squash merge release PR]
    O --> U[Tag merged release commit]
    U --> V[Trigger Release workflow]
  end

  subgraph release["Release (release.yml)"]
    V --> W[Push tag v*.*.*]
    W --> X[release job: verify version]
    X --> Y[Build matrix: linux-gnu, linux-musl, windows]
    Y --> Z[create-release: GitHub Release + artifacts]
    Z --> AA[update-package-managers]
    AA --> AB[Homebrew formula]
    AA --> AC[Scoop manifest]
  end

  subgraph cleanup["Cleanup"]
    M --> AD[cleanup-old-releases]
    AD --> AE[Delete merged release branches]
  end

  A --> B
  B --> I
```

## Validations and stages

### 1. CI workflow (`ci.yml`)

**Triggers:** `push` to `main`, `pull_request` to `main`.

| Stage           | What runs |
|-----------------|-----------|
| Checkout        | Repository checkout. |
| Rust toolchain  | `rustfmt`, `clippy` components. |
| Cache           | Cargo registry, git, `target`. |
| Format check    | `cargo fmt --all -- --check`. |
| Clippy          | `cargo clippy --all-targets --all-features -- -D warnings`. |
| Build           | `cargo build --release`. |
| Tests           | `cargo test --all-features`. |
| Upload artifact | Binary `newton` as `newton-linux-x86_64`. |
| Security audit  | Separate job: `cargo audit` (Rust advisory DB); `continue-on-error: true`. |

All of these must pass (other than the optional security audit) before merging.

### 2. Merge and Auto Release gate

After merge to `main`:

- **Paths ignored for Auto Release:** `.github/**`, `docs/**`, `README.md`, `CHANGELOG.md`. Pushes that only touch these do not run Auto Release.
- **Skip conditions:** The commit message contains `[skip release]`, or it is itself a `chore: release v...` commit.
- Auto Release derives the next unused patch version from the latest reachable tag and the current `Cargo.toml` version.

### 3. Auto Release workflow (`auto-release.yml`)

**Triggers:** `push` to `main` (with path and skip conditions above).

| Stage                 | What runs |
|-----------------------|-----------|
| Compute version       | Choose the next unused patch after the latest reachable tag and current `Cargo.toml` version. |
| Create release branch | Branch `auto-release-<next_version>` from main; delete an existing stale branch if present. |
| Bump patch            | Increment patch in `Cargo.toml` on the release branch. |
| Commit & push         | Commit version bump, push branch with `--force-with-lease`. |
| Create/update PR      | Open PR base `main` head release branch and label it `auto-release`; update it if already open. |
| Enable auto-merge     | Arm squash auto-merge. Required checks decide when the PR can land. |
| Tag release commit    | On the release commit's push to `main`, verify its subject and `Cargo.toml` version, then push its immutable `vX.Y.Z` tag. |
| Trigger Release       | The tag push triggers `release.yml`. |

**Cleanup job (same workflow):** On push to `main`, deletes remote branches for merged release PRs, or branches older than 7 days with no PR.

### 4. Release workflow (`release.yml`)

**Triggers:** `push` of tags matching `v*.*.*`.

| Stage              | What runs |
|--------------------|-----------|
| Checkout           | Repository at the tag ref. |
| Verify version     | Tag version (e.g. `v0.3.9`) must match `version` in `Cargo.toml`; fail otherwise. |
| Build (matrix)     | Three targets: `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `x86_64-pc-windows-msvc`; musl job installs `musl-tools`. |
| Create archives    | Per target: copy binary to `dist/`, create `.tar.gz` or `.zip`, upload artifact. |
| Create GitHub Release | Download all artifacts, generate release notes, create release for the tag with `artifacts/**/*`. |
| Update package managers | After successful release: update `gonewton/homebrew-cli` (formula) and `gonewton/scoop-bucket` (manifest) via scripts and push. |

### 5. Post-release checks

- **Homebrew:** Formula URL, version, and hash should match the new release.
- **Scoop:** Manifest should point at the new version and asset URLs.

---

## Deployment checklist

Use this checklist when shipping code through the pipeline.

### 1. Prepare the change locally

1. Verify the tree is clean: `git status -sb`.
2. Implement the feature/fix and update docs/tests.
3. Run the same checks as CI:
   - `cargo fmt`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo test --all-features`
4. Commit with a conventional message, e.g. `feat(cli): add new flag`.

### 2. Push and monitor CI

1. Open a PR to `main` or push to `main`.
2. Find the CI run: `gh run list --limit 5` (workflow `CI`).
3. Watch until it finishes: `gh run watch <CI_RUN_ID> --exit-status`.
4. If it fails, fix locally and repeat.

### 3. Track Auto Release

1. After merging to `main`, find the Auto Release run: `gh run list --limit 5` (workflow `Auto Release`).
2. Watch it: `gh run watch <AUTO_RELEASE_RUN_ID> --exit-status`.
3. A release PR will be created with auto-merge enabled. Its required checks must pass before the tag is created.

### 4. Ensure the Release workflow runs

1. Auto Release pushes a tag (e.g. `v0.3.9`). To re-tag after a fix: `git push origin :refs/tags/vX.Y.Z` then `git push origin vX.Y.Z`.
2. Confirm Release ran: `gh run list --workflow Release --limit 3`.
3. Watch the run: `gh run watch <RELEASE_RUN_ID> --exit-status`.
4. Verify the release: `gh release view vX.Y.Z`.

### 5. Validate package-manager updates

1. Homebrew formula (URL/version/hash):
   - `gh api repos/gonewton/homebrew-cli/contents/Formula/newton.rb?ref=main --jq '.content' | base64 -d`
2. Optionally check the Scoop manifest in `gonewton/scoop-bucket`.

### 6. Verify via Homebrew locally

1. `brew update`
2. `brew upgrade newton` (or `brew install newton`)
3. `newton --version` and `newton --help` (banner should show the new version).

### 7. Troubleshooting

- Inspect failures: `gh run view <RUN_ID> --log`.
- If Auto Release did not run: check the ignored paths and whether the commit contains `[skip release]` or is already a release commit.
- If Release fails on version mismatch: tag and `Cargo.toml` at that tag must match; fix version and re-tag.
- Release workflow runs on **tag push**; the ref used is the tag (so `Cargo.toml` at the tag must have the same version as the tag).
