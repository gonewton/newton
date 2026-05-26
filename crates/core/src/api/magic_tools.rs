use aikit_magictool::{ChatEvent, MagicToolState, MockChat, MockExecutor, ToolDef, ToolRegistry};
use serde_json::json;
use std::sync::Arc;

/// Build a MagicToolState with newton's own (initially empty) registry.
/// Part B registers ToolDefs here; do NOT call default_registry_state().
pub fn build_state() -> MagicToolState {
    let ping = ToolDef::new(
        "newton",
        "ping",
        "No-op smoke-test tool; returns {\"pong\": true}.",
        "Return {\"pong\": true}.",
        json!({ "type": "object" }),
        json!({
            "type": "object",
            "properties": { "pong": { "type": "boolean" } },
            "required": ["pong"],
            "additionalProperties": false
        }),
    );

    let mut registry = ToolRegistry::new();
    registry.register(ping);

    MagicToolState {
        registry: Arc::new(registry),
        executor: Arc::new(MockExecutor::ok(json!({"pong": true}))),
        chat: Some(Arc::new(MockChat::new(
            vec![
                ChatEvent::Started {
                    session_id: "ping-session".to_owned(),
                },
                ChatEvent::Final(r#"{"pong":true}"#.to_owned()),
            ],
            json!({"pong": true}),
        ))),
    }
}
