use std::sync::Arc;

use anyhow::anyhow;
use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;

use crate::cli::categories;
use crate::cli::exit::CliExit;
use crate::cli::framework_setup::error_codes;
use crate::cli::framework_setup::get_opt_path;
use crate::cli::framework_setup::get_opt_str;
use crate::cli::ops;

pub(crate) fn doctor_command() -> Command {
    Command {
        id: "doctor".into(),
        spec: Arc::new(CommandSpec {
            summary: "Run local environment diagnostic probes",
            syntax: Some("[OPTIONS]"),
            category: Some(categories::OPERATIONAL),
            long_about: Some(
                "Doctor runs a small set of probes (workspace, config, ailoop reachability, gh,\n\
                 logging) and prints one `OK|FAIL|SKIP <name>: <detail>` line per probe.\n\
                 Exits 0 if all probes pass, 1 if any fail.",
            ),
            examples: vec!["newton doctor", "newton doctor --workspace ./workspace"],
            args: vec![ArgSpec {
                name: "workspace",
                kind: ArgKind::Option,
                long: Some("workspace"),
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                help: "Workspace root to probe (defaults to CWD with .newton/)",
                ..Default::default()
            }],
            ..Default::default()
        }),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let workspace = get_opt_path(&args, "workspace");
                let report = ops::doctor::run(ops::doctor::DoctorArgs { workspace })?;
                report.print();
                if report.any_failed() {
                    return Err(CliExit::new(1, "doctor: one or more probes failed").into());
                }
                Ok(())
            })
        }),
        expose_mcp: false,
        expose_chat: true,
    }
}

pub(crate) fn config_command() -> Command {
    Command {
        id: "config".into(),
        spec: Arc::new(CommandSpec {
            summary: "Inspect resolved Newton configuration",
            syntax: Some("show [OPTIONS]"),
            category: Some(categories::OPERATIONAL),
            long_about: Some(
                "Config currently exposes one subcommand: `show`.\n\
                 `newton config show` prints the resolved configuration as JSON, with values\n\
                 whose key looks like a secret (token/secret/password/key) replaced by\n\
                 `***REDACTED***`.",
            ),
            examples: vec![
                "newton config show",
                "newton config show --workspace ./workspace",
            ],
            args: vec![
                ArgSpec {
                    name: "subcommand",
                    kind: ArgKind::Positional,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Subcommand: show (only supported value)",
                    ..Default::default()
                },
                ArgSpec {
                    name: "workspace",
                    kind: ArgKind::Option,
                    long: Some("workspace"),
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    help: "Workspace root (optional)",
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let sub = get_opt_str(&args, "subcommand").unwrap_or_else(|| "show".to_string());
                if sub != "show" {
                    return Err(anyhow!(
                        "{}: only `config show` is supported (got `config {}`)",
                        error_codes::CLI_MIG_001,
                        sub
                    ));
                }
                let workspace = get_opt_path(&args, "workspace");
                ops::config_show::run(ops::config_show::ConfigShowArgs { workspace })
            })
        }),
        expose_mcp: true,
        expose_chat: true,
    }
}
