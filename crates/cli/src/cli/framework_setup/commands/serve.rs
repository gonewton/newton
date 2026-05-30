use std::sync::Arc;

use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;

use crate::cli::args::ServeArgs;
use crate::cli::categories;
use crate::cli::commands;
use crate::cli::framework_setup::help_text::SERVE_LONG_ABOUT;

pub(crate) fn serve_command() -> Command {
    Command {
        id: "serve",
        summary: "Start the Newton HTTP API server",
        syntax: Some("[OPTIONS]"),
        category: Some(categories::OPS),
        spec: Some(Arc::new(CommandSpec {
            summary: "Start the Newton HTTP API server",
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
                    short: None,
                    long: Some("host"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Host address to bind the server to (default: 127.0.0.1)",
                },
                ArgSpec {
                    name: "port",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("port"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Port to listen on (default: 8080)",
                },
                ArgSpec {
                    name: "static-ui",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("static-ui"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path to the built Newton UI dist directory (optional)",
                },
                ArgSpec {
                    name: "with-mcp",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("with-mcp"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Mount the MCP HTTP router on the same listener as the Newton API",
                },
                ArgSpec {
                    name: "with-embedded-ailoop",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("with-embedded-ailoop"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Embed the ailoop HTTP/WebSocket server on the same listener as the Newton API",
                },
                ArgSpec {
                    name: "ailoop-base-path",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("ailoop-base-path"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Path prefix where the embedded ailoop router is mounted (default: /ailoop). Must not be `/api`.",
                },
                ArgSpec {
                    name: "state-dir",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("state-dir"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Override the state root directory where checkpoints, artifacts, and backend.sqlite are stored. Defaults to auto-resolved from workspace root.",
                },
                ArgSpec {
                    name: "import-existing",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("import-existing"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Run import scan of existing file-based runs before the HTTP listener binds",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = ServeArgs::try_from(args)?;
                commands::serve(dto).await.map_err(anyhow::Error::from)
            })
        }),
        expose_mcp: false,
    }
}
