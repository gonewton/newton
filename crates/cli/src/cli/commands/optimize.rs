//! Definition-bound native optimization. Legacy plan files are not consumed.

use crate::cli::args::OptimizeArgs;
use anyhow::{anyhow, Context};
use newton_core::optimization::{
    bind_definition, inspect_binding, parse_definition, DefinitionBinding, EnforcementCapabilities,
};
use newton_types::optimization::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

mod control;
mod envelopes;
mod lifecycle;
mod native;
mod ownership;
mod workflow;

/// Run or inspect/resume a durable, definition-bound native optimization loop.
pub async fn optimize(args: OptimizeArgs) -> crate::Result<()> {
    let workspace = args
        .workspace
        .clone()
        .unwrap_or(std::env::current_dir()?)
        .canonicalize()?;
    let state_dir = crate::cli::workspace_paths::resolve_state_dir(&workspace, None);
    if args.requirements_update.is_some() && args.resume.is_none() {
        anyhow::bail!("--requirements-update requires --resume <RUN_ID>");
    }
    if let Some(run_id) = &args.resume {
        uuid::Uuid::parse_str(run_id).context("resume requires an Optimize Run UUID")?;
        if args.definition.is_some() {
            anyhow::bail!("resume uses the persisted definition; do not pass --definition");
        }
        let mut journal: lifecycle::Journal = serde_json::from_slice(&fs::read(
            state_dir.join("optimize").join(run_id).join("journal.json"),
        )?)
        .context("load durable Optimize Run journal")?;
        if journal.run_id != *run_id {
            anyhow::bail!("Optimize Run journal identity mismatch");
        }
        if !control::apply_update(
            &mut journal,
            &state_dir,
            args.requirements_update.as_deref(),
        )
        .await?
        {
            println!("requirements update is Pending; reconcile recorded work before activation");
            return Ok(());
        }
        if journal.phase == lifecycle::Phase::Finished {
            println!("{}", serde_json::to_string_pretty(&journal.outcome)?);
            return Ok(());
        }
        let (events, _) = tokio::sync::broadcast::channel(128);
        let outcome = native::NativeDriver::resume(journal, state_dir, events)
            .await?
            .run(args.once, args.poll_interval_seconds)
            .await?;
        println!("{}", serde_json::to_string_pretty(&outcome)?);
        return Ok(());
    }
    let config_path = workspace
        .join(".newton/configs")
        .join(format!("{}.conf", args.project_id));
    let config = if config_path.exists() {
        newton_core::core::plan_queue_config::parse_conf(&config_path)?
    } else {
        Default::default()
    };
    let definition_path = args.definition.clone()
        .or_else(|| config.get("definition_file").map(|p| PathBuf::from(unquote(p))))
        .ok_or_else(|| anyhow!("native optimize requires --definition <PATH> or definition_file in {}; legacy plan queues are not consumed", config_path.display()))?;
    let definition_path = workspace
        .join(definition_path)
        .canonicalize()
        .context("resolve Optimization Definition")?;
    let definition = parse_definition(&fs::read_to_string(&definition_path)?)?;
    let context_root = config
        .get("project_root")
        .map(|p| workspace.join(unquote(p)))
        .unwrap_or_else(|| workspace.clone())
        .canonicalize()?;
    let project_parameters = config
        .iter()
        .filter_map(|(key, value)| {
            key.strip_prefix("parameter.").map(|name| {
                (
                    name.into(),
                    ParameterValue::Literal {
                        value: serde_json::from_str(value)
                            .unwrap_or_else(|_| serde_json::json!(unquote(value))),
                    },
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    let allowed_actions = config
        .get("optimize_allowed_actions")
        .map(|value| {
            unquote(value)
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|action| serde_json::from_value(serde_json::json!(action.trim())))
                .collect::<Result<BTreeSet<ExecutionAction>, _>>()
        })
        .transpose()
        .context("optimize_allowed_actions must be comma-separated action names")?
        .unwrap_or_default();
    let authority = ExecutionAuthority { allowed_actions };
    let binding = bind_definition(
        &definition,
        DefinitionBinding {
            run_id: uuid::Uuid::new_v4().to_string(),
            context: OptimizationContext {
                id: args.project_id,
                root: context_root.display().to_string(),
            },
            project_parameters,
            run_overrides: BTreeMap::new(),
            project_authority: authority.clone(),
            environment_authority: authority,
            enforcement: EnforcementCapabilities::default(),
        },
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&inspect_binding(&binding))?
    );
    fs::create_dir_all(&state_dir)?;
    let (events, _) = tokio::sync::broadcast::channel(128);
    let outcome = native::NativeDriver::start(
        binding,
        definition_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf(),
        state_dir,
        events,
    )
    .await?
    .run(args.once, args.poll_interval_seconds)
    .await?;
    println!("{}", serde_json::to_string_pretty(&outcome)?);
    Ok(())
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'')
}
