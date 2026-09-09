use crate::cli::{
    categories,
    commands::dependency,
    framework_setup::{get_opt_path, get_opt_str},
};
use anyhow::{anyhow, Result};
use cli_framework::{
    command::Command,
    spec::{
        arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality},
        command_tree::CommandSpec,
    },
};
use std::sync::Arc;

pub(crate) const GROUP_SUMMARY: &str = "Discover, review, and query approved dependency maps";

const INSPECT_HELP: &str = "Validate a dependency map and emit canonical JSON facts and the fingerprint a\nhuman must review. Accepts a raw DependencyMap or a document containing map.\nInspection never approves a map. No server or portfolio database is required.\n\nEXAMPLES:\n  newton dependency inspect --map examples/dependency-planning/map.json\nSee docs/dependency-planning.md for the complete map/review/baseline contract.";
const DISCOVER_HELP: &str = "Read one Cargo.toml using an explicit package/source catalog, refresh its Detected\nedges, and emit an UNAPPROVED review document. Declared and Suggested edges\nsurvive rediscovery. Missing packages and unsupported Cargo inputs appear as\nissues; empty issues never prove cross-service completeness. Only Cargo manifest\ndiscovery is supported; this does not scan lockfiles or resolve workspace inheritance.\nNo Cargo/Git commands or network requests are executed.\n\nEXAMPLES:\n  newton dependency discover --map examples/dependency-planning/map.json --manifest examples/dependency-planning/Cargo.toml --catalog examples/dependency-planning/catalog.json --owner app";
const APPROVE_HELP: &str = "Validate a human-issued review record against the exact map fingerprint and\ndurably create a Baseline JSON file. The review must identify the authorized human,\nreview time, completeness statement, and all acknowledged unresolved issue IDs.\nThis command packages an existing human approval; it does not authenticate the\nreviewer or grant agents authority to approve. Existing files are never overwritten.\nThis operation is deliberately not exposed to MCP or chat.\n\nEXAMPLES:\n  newton dependency approve --map reviewed-map.json --review human-review.json --output baseline.json\nSee docs/dependency-planning.md for a complete human-review.json template.";
const IMPACT_HELP: &str = "Compute a deterministic, target-scoped Impact Sequence from an approved Baseline.\nOnly Detected/Declared edges drive sequencing. Compatible hops remain included;\ncycles are explicit Co-release Groups. Unknown compatibility requires adaptation.\nOptional changes JSON contains project-assigned versions/signals keyed by Module\nID. Newton never assigns versions and never executes releases. Output is always JSON.\n\nEXAMPLES:\n  newton dependency impact --baseline baseline.json --changed base --target product\n  newton dependency impact --baseline baseline.json --changed base --target product --changes examples/dependency-planning/changes.json";

fn option(name: &'static str, required: bool, help: &'static str) -> ArgSpec {
    ArgSpec {
        name,
        kind: ArgKind::Option,
        long: Some(name),
        value_type: ArgValueType::String,
        cardinality: if required {
            Cardinality::Required
        } else {
            Cardinality::Optional
        },
        help,
        ..Default::default()
    }
}

pub(crate) fn commands() -> Vec<(&'static str, Command)> {
    ["discover", "inspect", "approve", "impact"]
        .into_iter()
        .map(|verb| (verb, command(verb)))
        .collect()
}

fn command(verb: &'static str) -> Command {
    let (summary, syntax, long_about, examples, args) = match verb {
        "inspect" => ("Inspect canonical dependency facts and review fingerprint", "--map FILE", INSPECT_HELP,
            vec!["newton dependency inspect --map examples/dependency-planning/map.json"],
            vec![option("map", true, "DependencyMap or prior review/baseline JSON document")]),
        "discover" => ("Discover Cargo manifest facts without approving the map", "--map FILE --manifest FILE --catalog FILE --owner MODULE [--ecosystem cargo]", DISCOVER_HELP,
            vec!["newton dependency discover --map examples/dependency-planning/map.json --manifest examples/dependency-planning/Cargo.toml --catalog examples/dependency-planning/catalog.json --owner app"],
            vec![option("map", true, "Existing dependency map/review document; declarations are preserved"),
                 option("manifest", true, "Path to one real Cargo.toml; no automatic workspace scanning"),
                 option("catalog", true, "JSON array of explicit CargoPackage identities and sources"),
                 option("owner", true, "Module ID owning the supplied Cargo manifest"),
                 ArgSpec { name: "ecosystem", kind: ArgKind::Option, long: Some("ecosystem"), value_type: ArgValueType::Enum(vec!["cargo"]), cardinality: Cardinality::Optional, help: "Supported adapter: cargo (default); other ecosystems fail explicitly", ..Default::default() }]),
        "approve" => ("Persist a Baseline from an existing human-issued review", "--map FILE --review FILE --output FILE", APPROVE_HELP,
            vec!["newton dependency approve --map reviewed-map.json --review human-review.json --output baseline.json"],
            vec![option("map", true, "Exact dependency map/review document reviewed by the human"),
                 option("review", true, "Human-issued BaselineApproval JSON with exact map fingerprint"),
                 option("output", true, "New immutable Baseline JSON file; existing paths are rejected")]),
        "impact" => ("Compute a deterministic Impact Sequence from an approved Baseline", "--baseline FILE --changed MODULE --target ARTIFACT [--changes FILE]", IMPACT_HELP,
            vec!["newton dependency impact --baseline baseline.json --changed base --target product",
                 "newton dependency impact --baseline baseline.json --changed base --target product --changes examples/dependency-planning/changes.json"],
            vec![option("baseline", true, "Human-approved BaselineDocument JSON; tampering or missing approval fails"),
                 option("changed", true, "Changed Module catalog ID"), option("target", true, "Target Product, Component, Repo, or Module catalog ID"),
                 option("changes", false, "JSON map of project-owned planned versions/compatibility; absent facts remain Unknown")]),
        _ => unreachable!("private command inventory"),
    };
    Command {
        id: verb.into(),
        spec: Arc::new(CommandSpec {
            summary,
            syntax: Some(syntax),
            category: Some(categories::WORKFLOW),
            long_about: Some(long_about),
            examples,
            args,
            ..Default::default()
        }),
        validator: None,
        execute: Arc::new(move |ctx, args| {
            Box::pin(async move {
                let path =
                    |name| get_opt_path(&args, name).ok_or_else(|| anyhow!("--{name} is required"));
                let string =
                    |name| get_opt_str(&args, name).ok_or_else(|| anyhow!("--{name} is required"));
                let output = match verb {
                    "inspect" => serde_json::to_value(dependency::inspect(&path("map")?)?)?,
                    "discover" => serde_json::to_value(dependency::discover(
                        &path("map")?,
                        &path("manifest")?,
                        &path("catalog")?,
                        &string("owner")?,
                    )?)?,
                    "approve" => serde_json::to_value(dependency::approve(
                        &path("map")?,
                        &path("review")?,
                        &path("output")?,
                    )?)?,
                    "impact" => serde_json::to_value(dependency::impact(
                        &path("baseline")?,
                        &string("changed")?,
                        &string("target")?,
                        get_opt_path(&args, "changes").as_deref(),
                    )?)?,
                    _ => unreachable!("private command inventory"),
                };
                ctx.framework_println(&serde_json::to_string_pretty(&output)?);
                ctx.framework_set_structured_content(output);
                Ok(()) as Result<()>
            })
        }),
        expose_mcp: matches!(verb, "inspect" | "impact"),
        expose_chat: matches!(verb, "inspect" | "impact"),
        meta: None,
        visibility: None,
    }
}
