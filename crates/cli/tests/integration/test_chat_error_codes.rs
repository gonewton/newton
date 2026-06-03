#[path = "../support/mod.rs"]
mod support;

use cli_framework::command::chat::host_tool_adapter::McpHostToolAdapter;
use cli_framework::command::chat::{
    ChatToolCallOptions, CHAT_AGENT_START_FAILED, CHAT_ARG_VALIDATION_FAILED,
    CHAT_COMMAND_EXECUTION_FAILED, CHAT_DESTRUCTIVE_BLOCKED, CHAT_RISK_REQUIRES_CONFIRMATION,
    CHAT_TOOL_NOT_FOUND,
};
use cli_framework::command::{Command, CommandRegistry};
use cli_framework::mcp::McpToolRegistry;
use cli_framework::security::command_risk::CommandRiskPolicy;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use serde_json::json;
use serial_test::serial;
use std::process::Output;
use std::sync::Arc;

fn combined_output(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn make_ok_command(id: &'static str, category: Option<&'static str>) -> Command {
    Command {
        id: id.into(),
        spec: Arc::new(CommandSpec {
            summary: "test",
            category,
            ..Default::default()
        }),
        validator: None,
        expose_mcp: true,
        execute: Arc::new(|_ctx, _args| Box::pin(async { Ok(()) })),
    }
}

fn make_err_command(id: &'static str, category: Option<&'static str>) -> Command {
    Command {
        id: id.into(),
        spec: Arc::new(CommandSpec {
            summary: "test",
            category,
            ..Default::default()
        }),
        validator: None,
        expose_mcp: true,
        execute: Arc::new(|_ctx, _args| Box::pin(async { Err(anyhow::anyhow!("boom")) })),
    }
}

fn make_needs_arg_command() -> Command {
    Command {
        id: "needs_arg".into(),
        spec: Arc::new(CommandSpec {
            summary: "needs arg",
            args: vec![ArgSpec {
                name: "name",
                kind: ArgKind::Option,
                long: Some("name"),
                value_type: ArgValueType::String,
                cardinality: Cardinality::Required,
                help: "name",
                ..Default::default()
            }],
            ..Default::default()
        }),
        validator: None,
        expose_mcp: true,
        execute: Arc::new(|_ctx, _args| Box::pin(async { Ok(()) })),
    }
}

#[test]
#[serial(chat_error_codes)]
fn chat_agent_start_failed_when_api_key_missing() {
    let mut cmd = support::newton();
    cmd.args(["chat", "-p", "hello"]);
    cmd.env_remove("OPENAI_API_KEY");
    cmd.env_remove("AIKIT_API_KEY");
    cmd.env_remove("AIKIT_LLM_URL");
    cmd.env_remove("AIKIT_MODEL");

    let output = cmd.output().expect("run newton");
    assert!(
        !output.status.success(),
        "expected failure, got: {:?}",
        output.status
    );
    let text = combined_output(&output);
    assert!(
        text.contains(CHAT_AGENT_START_FAILED),
        "expected {CHAT_AGENT_START_FAILED} in output, got:\n{text}"
    );
}

/// Helper: run a single `McpHostToolAdapter::call_tool` call on a fresh OS thread
/// with its own tokio runtime.
///
/// `McpHostToolAdapter::call_tool` uses `Handle::current().block_on()` internally.
/// That panics when called from an active tokio executor thread (which is what
/// `#[tokio::test]` creates). Spawning a thread with a new multi-thread runtime
/// satisfies the `block_on` requirement without nesting.
fn spawn_call_tool(
    tool_registry: Arc<McpToolRegistry>,
    opts: ChatToolCallOptions,
    tool_name: &'static str,
    args: serde_json::Value,
) -> Result<String, String> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        let _guard = rt.enter();
        let exec = McpHostToolAdapter::new(tool_registry, opts);
        exec.call_tool(tool_name, args)
    })
    .join()
    .expect("thread panicked")
}

#[test]
#[serial(chat_error_codes)]
fn chat_tool_error_codes_are_deterministic() {
    std::env::remove_var("ALLOW_DESTRUCTIVE_COMMANDS");

    let mut registry = CommandRegistry::new();
    registry.register(make_ok_command("safe", None));
    registry.register(make_needs_arg_command());
    registry.register(make_err_command("fails", None));
    registry.register(make_ok_command("sensitive", Some("config")));
    registry.register(make_ok_command("destructive", Some("destructive")));

    let policy = CommandRiskPolicy::default();
    let tool_registry = Arc::new(
        McpToolRegistry::from_command_registry(&registry, "newton").with_risk_policy(policy),
    );

    let base_opts = || ChatToolCallOptions {
        yolo: false,
        interactive: false,
        ailoop_client: None,
    };

    // Unknown tool id.
    let err = spawn_call_tool(
        Arc::clone(&tool_registry),
        base_opts(),
        "newton_not_real",
        json!({}),
    )
    .expect_err("should fail");
    assert!(err.contains(CHAT_TOOL_NOT_FOUND));

    // Invalid args for a known tool.
    let err = spawn_call_tool(
        Arc::clone(&tool_registry),
        base_opts(),
        "newton_needs_arg",
        json!({}),
    )
    .expect_err("should fail");
    assert!(err.contains(CHAT_ARG_VALIDATION_FAILED));

    // Underlying command failure.
    let err = spawn_call_tool(
        Arc::clone(&tool_registry),
        base_opts(),
        "newton_fails",
        json!({}),
    )
    .expect_err("should fail");
    assert!(err.contains(CHAT_COMMAND_EXECUTION_FAILED));

    // Sensitive command in non-interactive context.
    let err = spawn_call_tool(
        Arc::clone(&tool_registry),
        base_opts(),
        "newton_sensitive",
        json!({}),
    )
    .expect_err("should fail");
    assert!(err.contains(CHAT_RISK_REQUIRES_CONFIRMATION));

    // Destructive command blocked by env policy.
    let err = spawn_call_tool(
        Arc::clone(&tool_registry),
        base_opts(),
        "newton_destructive",
        json!({}),
    )
    .expect_err("should fail");
    assert!(err.contains(CHAT_DESTRUCTIVE_BLOCKED));
}

#[test]
#[serial(chat_error_codes)]
fn chat_tool_registry_slash_and_dot_are_distinct_tool_names() {
    // With cli-framework >= b9ebeb1, tool names use "_" separator (replace '/' -> '_').
    // "a/b" → "newton_a_b", "a.b" → "newton_a.b" — these are distinct, no collision.
    let mut registry = CommandRegistry::new();
    registry.register(make_ok_command("a/b", None));
    registry.register(make_ok_command("a.b", None));

    let exec = McpToolRegistry::from_command_registry(&registry, "newton");
    assert_eq!(
        exec.tool_count(),
        2,
        "slash and dot produce distinct tool names"
    );
}

// Note: Newton does not currently provide a `--no-default-features` build that disables
// `cli-framework`'s default `chat` feature. The framework still emits
// `CHAT_FEATURE_DISABLED` deterministically if `cli-framework` is compiled without `chat`.
