use std::sync::Arc;

use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;

use crate::cli::categories;
use crate::cli::commands::schema::{schema_export_cmd, SchemaExportArgs};
use crate::cli::framework_setup::{get_bool, get_opt_path, get_opt_str};

pub(crate) fn schema_command() -> Command {
    Command {
        id: "schema".into(),
        spec: Arc::new(CommandSpec {
            summary: "Export the composed workflow JSON Schema",
            syntax: Some("<export> [OPTIONS]"),
            category: Some(categories::WORKFLOW),
            long_about: None,
            examples: vec![
                "newton schema export",
                "newton schema export --pretty",
                "newton schema export --out workflow-schema.json --pretty",
            ],
            args: vec![
                ArgSpec {
                    name: "subcommand",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::Enum(vec!["export"]),
                    cardinality: Cardinality::Required,
                    help: "Subcommand: export",
                    ..Default::default()
                },
                ArgSpec {
                    name: "out",
                    kind: ArgKind::Option,
                    long: Some("out"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Output file path (defaults to stdout)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "pretty",
                    kind: ArgKind::Flag,
                    long: Some("pretty"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Pretty-print the JSON output",
                    ..Default::default()
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Workspace path (defaults to current directory)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let subcommand = get_opt_str(&args, "subcommand").unwrap_or_default();
                match subcommand.as_str() {
                    "export" => {
                        let dto = SchemaExportArgs {
                            out: get_opt_path(&args, "out"),
                            pretty: get_bool(&args, "pretty"),
                            workspace: get_opt_path(&args, "workspace"),
                        };
                        schema_export_cmd(dto).map_err(|e| anyhow::anyhow!("{e}"))
                    }
                    other => Err(anyhow::anyhow!("unknown schema subcommand: {other}")),
                }
            })
        }),
        expose_mcp: false,
        expose_chat: false,
    }
}
