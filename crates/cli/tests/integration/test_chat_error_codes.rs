#[path = "../support/mod.rs"]
mod support;

use cli_framework::command::chat::HostToolExecutor;
use cli_framework::command::chat::{
    ChatToolCallOptions, CommandsAsToolsExecutor, CHAT_AGENT_START_FAILED,
    CHAT_ARG_VALIDATION_FAILED, CHAT_COMMAND_EXECUTION_FAILED, CHAT_DESTRUCTIVE_BLOCKED,
    CHAT_RISK_REQUIRES_CONFIRMATION, CHAT_TOOL_NOT_FOUND, CHAT_TOOL_REGISTRY_COLLISION,
};
use cli_framework::command::{Command, CommandRegistry};
use cli_framework::security::command_risk::CommandRiskPolicy;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use newton_cli::cli::context::NewtonContext;
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

fn make_ok_command(
    id: &'static str,
    category: Option<&'static str>,
    spec: Option<CommandSpec>,
) -> Command {
    Command {
        id,
        summary: "test",
        syntax: None,
        category,
        spec: spec.map(Arc::new),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, _args| Box::pin(async { Ok(()) })),
    }
}

fn make_err_command(
    id: &'static str,
    category: Option<&'static str>,
    spec: Option<CommandSpec>,
) -> Command {
    Command {
        id,
        summary: "test",
        syntax: None,
        category,
        spec: spec.map(Arc::new),
        validator: None,
        expose_mcp: false,
        execute: Arc::new(|_ctx, _args| Box::pin(async { Err(anyhow::anyhow!("boom")) })),
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

#[tokio::test(flavor = "current_thread")]
#[serial(chat_error_codes)]
async fn chat_tool_error_codes_are_deterministic() {
    std::env::remove_var("ALLOW_DESTRUCTIVE_COMMANDS");

    let mut registry = CommandRegistry::new();
    registry.register(make_ok_command("safe", None, None));
    registry.register(make_ok_command(
        "needs_arg",
        None,
        Some(CommandSpec {
            summary: "needs arg",
            args: vec![ArgSpec {
                name: "name",
                kind: ArgKind::Option,
                short: None,
                long: Some("name"),
                value_type: ArgValueType::String,
                cardinality: Cardinality::Required,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "name",
            }],
            ..Default::default()
        }),
    ));
    registry.register(make_err_command("fails", None, None));
    registry.register(make_ok_command("sensitive", Some("config"), None));
    registry.register(make_ok_command("destructive", Some("destructive"), None));

    let policy = CommandRiskPolicy::default();
    let exec = CommandsAsToolsExecutor::new(&registry, "newton", policy).expect("tool executor");

    let opts = ChatToolCallOptions {
        yolo: false,
        interactive: false,
        ailoop_client: None,
    };

    let mut ctx = NewtonContext::new();

    // Unknown tool id.
    let err = exec
        .call_tool("newton.not_real", json!({}), &mut ctx, &opts)
        .await
        .expect_err("should fail");
    assert!(err.to_string().contains(CHAT_TOOL_NOT_FOUND));

    // Invalid args for a known tool.
    let err = exec
        .call_tool("newton.needs_arg", json!({}), &mut ctx, &opts)
        .await
        .expect_err("should fail");
    assert!(err.to_string().contains(CHAT_ARG_VALIDATION_FAILED));

    // Underlying command failure.
    let err = exec
        .call_tool("newton.fails", json!({}), &mut ctx, &opts)
        .await
        .expect_err("should fail");
    assert!(err.to_string().contains(CHAT_COMMAND_EXECUTION_FAILED));

    // Sensitive command in non-interactive context.
    let err = exec
        .call_tool("newton.sensitive", json!({}), &mut ctx, &opts)
        .await
        .expect_err("should fail");
    assert!(err.to_string().contains(CHAT_RISK_REQUIRES_CONFIRMATION));

    // Destructive command blocked by env policy.
    let err = exec
        .call_tool("newton.destructive", json!({}), &mut ctx, &opts)
        .await
        .expect_err("should fail");
    assert!(err.to_string().contains(CHAT_DESTRUCTIVE_BLOCKED));
}

#[test]
#[serial(chat_error_codes)]
fn chat_tool_registry_collision_is_reported() {
    let mut registry = CommandRegistry::new();
    registry.register(make_ok_command("a/b", None, None));
    registry.register(make_ok_command("a.b", None, None));

    let err = match CommandsAsToolsExecutor::new(&registry, "newton", CommandRiskPolicy::default())
    {
        Ok(_) => panic!("expected collision"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains(CHAT_TOOL_REGISTRY_COLLISION),
        "expected {CHAT_TOOL_REGISTRY_COLLISION} in error, got:\n{err}"
    );
}

// Note: Newton does not currently provide a `--no-default-features` build that disables
// `cli-framework`'s default `chat` feature. The framework still emits
// `CHAT_FEATURE_DISABLED` deterministically if `cli-framework` is compiled without `chat`.
