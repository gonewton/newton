use std::sync::Arc;

use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;

use crate::cli::args::OptimizeArgs;
use crate::cli::categories;
use crate::cli::commands;
use crate::cli::framework_setup::help_text::OPTIMIZE_LONG_ABOUT;
use crate::cli::framework_setup::FromArgValueMap;

pub(crate) fn optimize_command() -> Command {
    Command {
        id: "optimize".into(),
        spec: Arc::new(CommandSpec {
            summary: "Drive a project's optimization loop",
            syntax: Some("<PROJECT_ID> [OPTIONS]"),
            category: Some(categories::OPS),
            long_about: Some(OPTIMIZE_LONG_ABOUT),
            examples: vec![
                "newton optimize project-alpha",
                "newton optimize project-alpha --workspace ./workspace",
                "newton optimize project-alpha --once",
                "newton optimize project-alpha --definition ./security.yaml --once",
                "newton optimize project-alpha --resume <RUN_ID>",
            ],
            args: vec![
                ArgSpec {
                    name: "param", kind: ArgKind::Option, long: Some("param"),
                    value_type: ArgValueType::String, cardinality: Cardinality::Repeated,
                    help: "Non-secret NAME=JSON override; repeat for multiple values (defaults < project < run)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "inspect", kind: ArgKind::Flag, long: Some("inspect"),
                    value_type: ArgValueType::Bool, cardinality: Cardinality::Optional,
                    help: "Print resolved requirements without executing workflows or creating a run",
                    ..Default::default()
                },
                ArgSpec {
                    name: "preflight", kind: ArgKind::Flag, long: Some("preflight"),
                    value_type: ArgValueType::Bool, cardinality: Cardinality::Optional,
                    help: "Check workflow and evaluator prerequisites without starting a run",
                    ..Default::default()
                },
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
                    name: "definition",
                    kind: ArgKind::Option,
                    long: Some("definition"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Optimization Definition YAML; overrides project definition_file",
                    ..Default::default()
                },
                ArgSpec {
                    name: "resume",
                    kind: ArgKind::Option,
                    long: Some("resume"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Resume a persisted Optimize Run; uncertain external effects require reconciliation",
                    ..Default::default()
                },
                ArgSpec {
                    name: "requirements-update",
                    kind: ArgKind::Option,
                    long: Some("requirements-update"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "RequirementsUpdate YAML (requires --resume); owner activates at a safe evaluation boundary and regrades before acceptance",
                    ..Default::default()
                },
                ArgSpec {
                    name: "once",
                    kind: ArgKind::Flag,
                    long: Some("once"),
                    value_type: ArgValueType::Bool,
                    cardinality: Cardinality::Optional,
                    help: "Run one complete cycle including evaluation before acceptance",
                    ..Default::default()
                },
                ArgSpec {
                    name: "poll-interval",
                    kind: ArgKind::Option,
                    long: Some("poll-interval"),
                    value_type: ArgValueType::Int,
                    cardinality: Cardinality::Optional,
                    help: "Seconds between optimization cycles (default: 60)",
                    min: Some(1),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = OptimizeArgs::from_arg_value_map(&args);
                commands::optimize(dto).await
            })
        }),
        expose_mcp: false,
        expose_chat: false,
        meta: None,
        visibility: None,
    }
}
