# 048 - Migrate Newton ailoop integration from HTTP `ToolClient` to `ailoop-core` crate

## Purpose

Newton's human-in-the-loop and tooling integration today uses an in-tree **HTTP REST** client (`crates/core/src/integrations/ailoop/tool_client.rs`, `reqwest`) against paths such as `/questions/{channel}`, `/authorization/{channel}`, and `/notifications/{channel}`.

The **ailoop CLI** and the intended product surface instead use **`ailoop_core::client`** (`goailoop/ailoop` / crate **`ailoop-core`**), built on **WebSocket** helpers (`crate::transport::websocket::send_message_and_wait_response`), with a coherent message model (`Message`, `MessageContent::*`).

This spec defines migrating Newton to **consume `ailoop-core` as a library**, replacing the duplicated HTTP abstraction with the same primitives the CLI uses, improving parity and encapsulation.

**Related:** Draft **047** (`047-human-in-loop-always-via-ailoop`, human presentation policy via ailoop). This draft focuses on **which Rust API** Newton uses to talk to ailoop, not whether fallback to console remains.

## Recent revert (HTTP workaround)

Commit **`f265e13`** ("fix(ailoop): join HTTP tool paths without double slash on base URL") was **reverted** in **`63d19c0`**. Until this migration lands, deployments that configure `NEWTON_AILOOP_HTTP_URL` with a **trailing slash** (e.g. `http://127.0.0.1:8080/`) can still produce malformed URLs such as `//authorization/...`; operators SHOULD use **`http://127.0.0.1:8080`** (no trailing slash) or complete this migration early.

## Current state

| Capability | Rust entry | Transport |
|-----------|-------------|-----------|
| Approval | `ToolClient::request_authorization` | HTTP POST `{http}/authorization/{channel}` |
| Question / choices | `ToolClient::ask_question`, `ask_question_with_choices` | HTTP POST `{http}/questions/{channel}` |
| Notification | `ToolClient::send_notification` | HTTP POST `{http}/notifications/{channel}` |
| Gh optional gate | `AiloopSdkApprover` wrapping `ToolClient` | Same HTTP |

Consumers: `AiloopInterviewer` (`workflow/human/ailoop.rs`), `AiloopSdkApprover` (`integrations/ailoop/approver.rs`), and any callers of `ToolClient` / `OrchestratorNotifier` / `OutputForwarder` as wired in `integrations/ailoop/*.rs`.

## Target state

| Capability | Preferred Rust API | Notes |
|-----------|---------------------|-------|
| Approval | `ailoop_core::client::authorize(server_url, channel, action, timeout_secs)` | Uses WebSocket stack in `ailoop-core` |
| Question / choices | `ailoop_core::client::ask(server_url, channel, question, timeout_secs, choices)` | `choices: Option<Vec<String>>` |
| Notification | `ailoop_core::client::say(server_url, channel, text, priority_str)` | Map `NotificationLevel` to ailoop priority strings |

**Configuration:** Prefer a single **WS base URL** for these calls (Newton already parses `NEWTON_AILOOP_WS_URL` in `integrations/ailoop/config.rs`). The HTTP URL may remain only if other Newton features genuinely require REST against ailoop's HTTP tier; otherwise document deprecation for HIL-related traffic.

### Authorization "details" vs ailoop messages

Newton's HTTP payload sends **`action`** and **`details`**. Public `authorize()` in **`ailoop_core::client`** today builds:

```rust
MessageContent::Authorization {
    action: action.to_string(),
    context: None,
    ...
}
```

`MessageContent::Authorization` in **`ailoop-core`** supports **`context: Option<serde_json::Value>`**. Migration SHOULD either:

1. Extend **`goailoop/ailoop`** with an `authorize_with_context(...)`, **or**
2. Pass long-form **`details`** as JSON in **`context`** (if server/UI already renders it), **or**
3. Concatenate **`action` + `details`** into a single **`action`** string (minimal change, weakest UX).

Record the chosen strategy in implementation PR notes.

### Dependency impact (risk)

The **`ailoop-core`** crate is a **_workspace member_ package** (`ailoop-core/`) pulling **server-facing** crates (for example **`warp`**, **`crossterm`**) alongside client helpers. Adding it to **`newton-core`** expands the dependency graph. Follow-up MAY propose upstream splitting **`ailoop-client`** as a slim dependency (optional open question for `goailoop/ailoop` maintainers).

## How to fetch the **`ailoop` project** (public)

Upstream repository (**public**):

- **HTTPS clone:** `https://github.com/goailoop/ailoop.git`

```bash
git clone https://github.com/goailoop/ailoop.git
cd ailoop
```

Rust workspace layout: **`ailoop-core/`** is the crate Newton should depend on (**package name **`ailoop-core`**). **`ailoop-cli/`** is the binary crate.

### Add **`ailoop-core`** to **`gonewton/newton`**

Until the team pins a **crates.io** release (publish `ailoop-core` separately if desired), declare a **Git** dependency so CI is reproducible.

**Recommended (workspace dependency, pinned rev):**

In root **`Cargo.toml`** **`[workspace.dependencies]`**:

```toml
ailoop-core = { git = "https://github.com/goailoop/ailoop.git", package = "ailoop-core", rev = "<full-git-sha>" }
```

**Tag pinning** when upstream tags releases covering `ailoop-core`:

```toml
ailoop-core = { git = "https://github.com/goailoop/ailoop.git", package = "ailoop-core", tag = "<tag>" }
```

In **`crates/core/Cargo.toml`**:

```toml
ailoop-core = { workspace = true }
```

**Resolve version skew** (`tokio`, `tokio-tungstenite`, `tungstenite`, `serde`) by aligning **`[workspace.dependencies]`** with **`goailoop/ailoop` workspace** manifests; iterate with **`cargo tree -p newton-core -i tokio`** until builds are clean.

**After publish:**

```toml
ailoop-core = "x.y.z"
```

**Verify locally:**

```bash
cargo fetch
cargo check -p newton-core
```

Commit **`Cargo.lock`** with the pinned **`rev`**.

## Migration steps (recommended order)

1. Add **`ailoop-core`** dependency (git + **`rev`**), unify crate versions (`tokio`, `serde`, `url`).
2. Implement **`NewtonAiloopClient`** (thin internal façade) wrapping:
   - `authorize` - map `Option<Message>` into Newton **`AuthorizationResponse`** (`authorized`, `timed_out`, `reason`).
   - `ask` - map into **`QuestionResponse`**.
   - `say` - map failures into **`ClientError`** or a renamed shared error enum.
3. Replace **`ToolClient`** body calls with the façade (optionally rename type in follow-up PR).
4. Update **`AiloopInterviewer`** wires; drop **`reqwest`** from human-only paths where unused elsewhere.
5. Update **`integrations/ailoop/config.rs`** docs: **`NEWTON_AILOOP_WS_URL`** required for WS client; **`NEWTON_AILOOP_HTTP_URL`** optional or deprecated where superseded.
6. **Tests:** adapt **`integrations/ailoop` wiremock suite** where HTTP is removed; keep or extend **`tests/integration/test_human_ailoop.rs`**.
7. Remove dead HTTP paths once unused; rewrite **`approver.rs`** outdated **`ailoop-sdk`** comment into **`ailoop-core`**.
8. **Final cleanup (MUST):** When the migration is complete, **remove dead code** under **`crates/core/src/integrations/ailoop/`** (unused modules, helpers, `ToolClient` surface and types that no longer have callers, obsolete env/config branches, and tests that only covered removed HTTP paths). Drop **`reqwest`** from **`newton-core`** if nothing else in the crate needs it. Goal: no leftover HTTP tool-client implementation or orphan exports in that tree.

## Non-goals (this spec)

- Changing ailoop server HTTP route semantics independently of Newton.
- Implementing **[047]** console-removal tasks (orthogonal).

## Acceptance criteria

- [ ] **`newton-core`** depends on **`ailoop-core`** from **`https://github.com/goailoop/ailoop`** with pinned **`rev`** or semver.
- [ ] **`HumanApprovalOperator`** / **`HumanDecisionOperator`** (via **`AiloopInterviewer`**) call **`ailoop_core::client`**, not **`reqwest`** tool HTTP paths listed above.
- [ ] **`AiloopSdkApprover`** uses same stack or documents a narrow HTTP exception.
- [ ] **`cargo test`** passes; **`Cargo.lock`** updated.
- [ ] **`integrations/ailoop`** contains **no dead code** left from the superseded HTTP path (removed modules/helpers/tests/orphans per migration step **8**).
- [ ] Operator/integration docs describe clone URL, dependency snippets, **`WS`** URL expectation.

## References

- [`goailoop/ailoop/ailoop-core/src/client/mod.rs`](https://github.com/goailoop/ailoop/blob/main/ailoop-core/src/client/mod.rs) - `ask`, `authorize`, `say`
- [`goailoop/ailoop/ailoop-core/src/models/message.rs`](https://github.com/goailoop/ailoop/blob/main/ailoop-core/src/models/message.rs) - `MessageContent::Authorization`
- `gonewton/newton/crates/core/src/integrations/ailoop/tool_client.rs` - current HTTP implementation
- `gonewton/newton/crates/core/src/workflow/human/ailoop.rs` - interviewer glue
