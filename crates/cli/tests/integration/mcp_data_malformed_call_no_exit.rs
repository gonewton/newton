//! Spec 074, PR-1 / B3: a malformed `newton data` invocation dispatched over
//! the MCP tool-call path must return a structured error frame, not exit the
//! process. Handlers previously called `std::process::exit(1)` on validation
//! failure (see `crates/cli/src/cli/commands/data.rs`); one bad tool call
//! would have taken down the whole `newton serve --with-mcp` process.
//!
//! This test exercises the exact seam `newton serve --with-mcp` uses —
//! `cli_framework::mcp::dispatch_tool_call` over the same `McpToolRegistry`
//! Newton builds for MCP export (`build_mcp_command_registry`) — in-process.
//! If a regression reintroduced `std::process::exit` on this path, this test
//! process would be killed outright rather than merely failing an assertion,
//! which is a strictly stronger signal than a subprocess exit-code check.
//!
//! Prior art: `mcp_tool_call_health.rs` (registry construction),
//! `serve_with_mcp_responds.rs` (surface-level MCP reachability),
//! `test_chat_error_codes.rs` (in-process tool dispatch against a real
//! `McpToolRegistry`).
use cli_framework::mcp::{
    dispatch_tool_call, McpToolExportPolicy, McpToolRegistry, McpTransportKind,
};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

/// Mirrors `test_e2e_data.rs::setup_workspace_with_db` — a workspace whose
/// `.newton/state/` directory exists so `SqliteBackendStore::new` (which
/// opens with `mode=rwc`) can create `backend.sqlite` and run migrations.
fn setup_workspace_with_db() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".newton/state")).unwrap();
    dir
}

fn tool_registry() -> Arc<McpToolRegistry> {
    let registry = newton_cli::cli::framework_setup::build_mcp_command_registry()
        .expect("build_mcp_command_registry should succeed");
    Arc::new(McpToolRegistry::from_command_registry_with_policy(
        &registry,
        "newton",
        McpToolExportPolicy::ExposeMcpOnly,
    ))
}

#[tokio::test]
async fn malformed_data_call_returns_error_frame_and_server_keeps_serving() {
    let ws = setup_workspace_with_db();
    let registry = tool_registry();

    // 1. Malformed call: `resource` is not one of the resources `newton data`
    //    understands. This is exactly the DATA-003 validation path that used
    //    to call `std::process::exit(1)` inside the handler
    //    (crates/cli/src/cli/commands/data.rs).
    let bad_args = json!({
        "resource": "not-a-real-resource",
        "workspace": ws.path().to_string_lossy(),
    })
    .as_object()
    .cloned();

    let bad_result = dispatch_tool_call(
        &registry,
        "newton_data_get",
        bad_args,
        McpTransportKind::Http,
    )
    .await;

    let err = bad_result.expect_err("malformed data call must return an MCP error, not Ok");
    assert!(
        err.message.contains("DATA-003"),
        "expected DATA-003 in MCP error frame message, got: {}",
        err.message
    );
    assert!(
        err.message.contains("MCP_EXECUTION_FAILED"),
        "expected the standard MCP_EXECUTION_FAILED wrapper, got: {}",
        err.message
    );

    // 2. The whole point of B3: this test process is still alive to make a
    //    second call, and that second call succeeds normally. A handler that
    //    still called `std::process::exit` would have killed this test
    //    process on step 1 rather than merely failing the assertion above.
    let good_args = json!({
        "resource": "products",
        "workspace": ws.path().to_string_lossy(),
    })
    .as_object()
    .cloned();

    let good_result = dispatch_tool_call(
        &registry,
        "newton_data_get",
        good_args,
        McpTransportKind::Http,
    )
    .await;

    good_result.expect("subsequent well-formed call must still succeed after a bad call");
}

#[tokio::test]
async fn malformed_data_call_missing_id_returns_error_frame() {
    // A second malformed-call shape (DATA-002: missing required ID for a
    // single-item GET) to prove the conversion covers more than one exit
    // site, not just DATA-003.
    let ws = setup_workspace_with_db();
    let registry = tool_registry();

    let bad_args = json!({
        "resource": "product",
        "workspace": ws.path().to_string_lossy(),
    })
    .as_object()
    .cloned();

    let bad_result = dispatch_tool_call(
        &registry,
        "newton_data_get",
        bad_args,
        McpTransportKind::Http,
    )
    .await;

    let err = bad_result.expect_err("missing id for single-item GET must return an MCP error");
    assert!(
        err.message.contains("DATA-002"),
        "expected DATA-002 in MCP error frame message, got: {}",
        err.message
    );
}
