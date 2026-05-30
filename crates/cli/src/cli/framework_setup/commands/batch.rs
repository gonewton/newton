use std::sync::Arc;

use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;

use crate::cli::args::BatchArgs;
use crate::cli::categories;
use crate::cli::commands;
use crate::cli::framework_setup::help_text::BATCH_LONG_ABOUT;

pub(crate) fn batch_command() -> Command {
    Command {
        id: "batch",
        summary: "Process queued work items for a project",
        syntax: Some("<PROJECT_ID> [OPTIONS]"),
        category: Some(categories::OPS),
        spec: Some(Arc::new(CommandSpec {
            summary: "Process queued work items for a project",
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
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Required,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Project identifier that maps to .newton/configs/<project_id>.conf",
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Workspace root containing the .newton directory",
                },
                ArgSpec {
                    name: "once",
                    kind: ArgKind::Flag,
                    short: None,
                    long: Some("once"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Process a single plan and exit instead of running as a daemon",
                },
                ArgSpec {
                    name: "poll-interval",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("poll-interval"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Seconds to wait when the queue is empty (default: 60)",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = BatchArgs::try_from(args)?;
                commands::batch(dto).await
            })
        }),
        expose_mcp: false,
    }
}
