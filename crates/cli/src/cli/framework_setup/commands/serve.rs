use std::sync::Arc;

use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;

use crate::cli::args::ServeArgs;
use crate::cli::categories;
use crate::cli::commands;
use crate::cli::framework_setup::help_text::SERVE_LONG_ABOUT;
use crate::cli::framework_setup::FromArgValueMap;

pub(crate) fn serve_command() -> Command {
    Command {
        id: "serve".into(),
        spec: Arc::new(CommandSpec {
            summary: "Start the Newton HTTP API server",
            syntax: Some("[OPTIONS]"),
            category: Some(categories::OPS),
            long_about: Some(SERVE_LONG_ABOUT),
            examples: vec![
                "newton serve",
                "newton serve --host 0.0.0.0 --port 9000",
                "newton serve --static-ui ./ui/dist",
            ],
            args: vec![
                ArgSpec {
                    name: "host",
                    kind: ArgKind::Option,
                    long: Some("host"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Host address to bind the server to (default: 127.0.0.1)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "port",
                    kind: ArgKind::Option,
                    long: Some("port"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    help: "Port to listen on (default: 8080)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "static-ui",
                    kind: ArgKind::Option,
                    long: Some("static-ui"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Path to the built Newton UI dist directory (optional)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "with-mcp",
                    kind: ArgKind::Flag,
                    long: Some("with-mcp"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Mount the MCP HTTP router on the same listener as the Newton API",
                    ..Default::default()
                },
                ArgSpec {
                    name: "with-embedded-ailoop",
                    kind: ArgKind::Flag,
                    long: Some("with-embedded-ailoop"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Embed the ailoop HTTP/WebSocket server on the same listener as the Newton API",
                    ..Default::default()
                },
                ArgSpec {
                    name: "ailoop-base-path",
                    kind: ArgKind::Option,
                    long: Some("ailoop-base-path"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Path prefix where the embedded ailoop router is mounted (default: /ailoop). Must not be `/api`.",
                    ..Default::default()
                },
                ArgSpec {
                    name: "state-dir",
                    kind: ArgKind::Option,
                    long: Some("state-dir"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Override the state root directory where checkpoints, artifacts, and backend.sqlite are stored. Defaults to auto-resolved from workspace root.",
                    ..Default::default()
                },
                ArgSpec {
                    name: "import-existing",
                    kind: ArgKind::Flag,
                    long: Some("import-existing"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Run import scan of existing file-based runs before the HTTP listener binds",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = ServeArgs::from_arg_value_map(&args);
                commands::serve(dto).await.map_err(anyhow::Error::from)
            })
        }),
        expose_mcp: false,
        expose_chat: false,
    }
}
