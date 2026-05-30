use std::sync::Arc;

use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;

use crate::cli::args::InitArgs;
use crate::cli::categories;
use crate::cli::framework_setup::help_text::INIT_LONG_ABOUT;
use crate::cli::init;

pub(crate) fn init_command() -> Command {
    Command {
        id: "init",
        summary: "Initialize a Newton workspace with the default template",
        syntax: Some("[PATH] [OPTIONS]"),
        category: Some(categories::WORKSPACE),
        spec: Some(Arc::new(CommandSpec {
            summary: "Initialize a Newton workspace with the default template",
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
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help:
                        "Directory where .newton/ will be created (defaults to current directory)",
                },
                ArgSpec {
                    name: "template",
                    kind: ArgKind::Option,
                    short: None,
                    long: Some("template"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Template source (GitHub repo, URL, or local path)",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let dto = InitArgs::try_from(args)?;
                init::run(dto)
            })
        }),
        expose_mcp: false,
    }
}
