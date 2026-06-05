use std::sync::Arc;

use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;

use crate::cli::args::InitArgs;
use crate::cli::categories;
use crate::cli::framework_setup::help_text::INIT_LONG_ABOUT;
use crate::cli::framework_setup::FromArgValueMap;
use crate::cli::init;

pub(crate) fn init_command() -> Command {
    Command {
        id: "init".into(),
        spec: Arc::new(CommandSpec {
            summary: "Initialize a Newton workspace with the default template",
            syntax: Some("[PATH] [OPTIONS]"),
            category: Some(categories::WORKSPACE),
            long_about: Some(INIT_LONG_ABOUT),
            examples: vec![
                "newton init .",
                "newton init ./workspace",
                "newton init . --template gonewton/newton-templates",
            ],
            args: vec![
                ArgSpec {
                    name: "path",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help:
                        "Directory where .newton/ will be created (defaults to current directory)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "template",
                    kind: ArgKind::Option,
                    long: Some("template"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Template source (GitHub repo, URL, or local path)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = InitArgs::from_arg_value_map(&args);
                init::run(dto)
            })
        }),
        expose_mcp: false,
        expose_chat: false,
    }
}
