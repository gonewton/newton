use std::sync::Arc;

use anyhow::anyhow;
use cli_framework::command::Command;
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;

use crate::cli::categories;
use crate::cli::framework_setup::error_codes;
use crate::cli::framework_setup::get_opt_path;
use crate::cli::ops;

pub(crate) fn health_command() -> Command {
    Command {
        id: "health",
        summary: "Print a one-line liveness status",
        syntax: Some("[OPTIONS]"),
        category: Some(categories::OPERATIONAL),
        spec: Some(Arc::new(CommandSpec {
            summary: "Print a one-line liveness status",
            long_about: Some(
                "Health prints `newton OK <version>` and exits 0 if the binary can run.\n\
                 No workspace, network, or config access — suitable for container probes.",
            ),
            examples: vec!["newton health"],
            args: vec![],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, _args| Box::pin(async move { ops::health::run() })),
        expose_mcp: true,
    }
}

pub(crate) fn doctor_command() -> Command {
    Command {
        id: "doctor",
        summary: "Run local environment diagnostic probes",
        syntax: Some("[OPTIONS]"),
        category: Some(categories::OPERATIONAL),
        spec: Some(Arc::new(CommandSpec {
            summary: "Run local environment diagnostic probes",
            long_about: Some(
                "Doctor runs a small set of probes (workspace, config, ailoop reachability, gh,\n\
                 logging) and prints one `OK|FAIL|SKIP <name>: <detail>` line per probe.\n\
                 Exits 0 if all probes pass, 1 if any fail.",
            ),
            examples: vec!["newton doctor", "newton doctor --workspace ./workspace"],
            args: vec![ArgSpec {
                name: "workspace",
                kind: ArgKind::Option,
                short: None,
                long: Some("workspace"),
                value_type: ArgValueType::String,
                cardinality: Cardinality::Optional,
                default: None,
                conflicts_with: vec![],
                requires: vec![],
                help: "Workspace root to probe (defaults to CWD with .newton/)",
            }],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let workspace = get_opt_path(&args, "workspace");
                let report = ops::doctor::run(ops::doctor::DoctorArgs { workspace })?;
                report.print();
                if report.any_failed() {
                    std::process::exit(1);
                }
                Ok(())
            })
        }),
        expose_mcp: false,
    }
}

pub(crate) fn config_command() -> Command {
    Command {
        id: "config",
        summary: "Inspect resolved Newton configuration",
        syntax: Some("show [OPTIONS]"),
        category: Some(categories::OPERATIONAL),
        spec: Some(Arc::new(CommandSpec {
            summary: "Inspect resolved Newton configuration",
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
                    short: None,
                    long: None,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Subcommand: show (only supported value)",
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
                    help: "Workspace root (optional)",
                },
            ],
            ..Default::default()
        })),
        validator: None,
        execute: Arc::new(|_ctx, args| {
            Box::pin(async move {
                let sub = args
                    .named
                    .get("subcommand")
                    .cloned()
                    .or_else(|| args.positional.first().cloned())
                    .unwrap_or_else(|| "show".to_string());
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
    }
}
