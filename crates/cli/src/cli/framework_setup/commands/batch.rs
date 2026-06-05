use std::sync::Arc;

use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;

use crate::cli::args::BatchArgs;
use crate::cli::categories;
use crate::cli::commands;
use crate::cli::framework_setup::help_text::BATCH_LONG_ABOUT;
use crate::cli::framework_setup::FromArgValueMap;

pub(crate) fn batch_command() -> Command {
    Command {
        id: "batch".into(),
        spec: Arc::new(CommandSpec {
            summary: "Process queued work items for a project",
            syntax: Some("<PROJECT_ID> [OPTIONS]"),
            category: Some(categories::OPS),
            long_about: Some(BATCH_LONG_ABOUT),
            examples: vec![
                "newton batch project-alpha",
                "newton batch project-alpha --workspace ./workspace",
                "newton batch project-alpha --once",
                "newton batch project-alpha --poll-interval 30",
            ],
            args: vec![
                ArgSpec {
                    name: "project-id",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    help: "Project identifier that maps to .newton/configs/<project_id>.conf",
                    ..Default::default()
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Workspace root containing the .newton directory",
                    ..Default::default()
                },
                ArgSpec {
                    name: "once",
                    kind: ArgKind::Flag,
                    long: Some("once"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Process a single plan and exit instead of running as a daemon",
                    ..Default::default()
                },
                ArgSpec {
                    name: "poll-interval",
                    kind: ArgKind::Option,
                    long: Some("poll-interval"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    help: "Seconds to wait when the queue is empty (default: 60)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = BatchArgs::from_arg_value_map(&args);
                commands::batch(dto).await
            })
        }),
        expose_mcp: false,
        expose_chat: false,
    }
}
