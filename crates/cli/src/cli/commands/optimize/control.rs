//! Local, serialized requirements control. An inbox is not an active revision.

use super::{
    lifecycle::{Journal, Phase},
    ownership::RunClaim,
};
use anyhow::{anyhow, Context, Result};
use newton_core::optimization::{
    acknowledge_revision, propose_revision, EnforcementCapabilities, RequirementsUpdateAuthority,
    RevisionBoundary,
};
use newton_types::{optimization::*, BackendStore, PatchOptimizeRunBody};
use std::{fs, io::Write, path::Path};

pub(super) async fn apply_update(
    journal: &mut Journal,
    state_dir: &Path,
    update: Option<&Path>,
) -> Result<bool> {
    let directory = state_dir.join("optimize").join(&journal.run_id);
    let inbox = directory.join("requirements-pending.json");
    if update.is_none() && !inbox.exists() {
        return Ok(true);
    }
    let authority = RequirementsUpdateAuthority {
        actor: format!("local-cli-process:{}", std::process::id()),
        may_update_requirements: true,
        may_update_evaluators: true,
        may_update_execution_restrictions: true,
    };
    let enforcement = EnforcementCapabilities::default();
    let binding: BoundOptimizationDefinition = serde_json::from_value(journal.binding.clone())?;
    if let Some(path) = update {
        let update: RequirementsUpdate = serde_yaml::from_str(&fs::read_to_string(path)?)
            .context("parse declarative RequirementsUpdate YAML")?;
        let pending = match propose_revision(
            &binding.requirements,
            &binding.authority_ceiling,
            update.clone(),
            &authority,
            &enforcement,
        ) {
            Ok(pending) => pending,
            Err(error) => {
                record_rejection(
                    &directory,
                    &serde_json::to_value(update)?,
                    &error.to_string(),
                )?;
                return Err(error.into());
            }
        };
        // Never mutate a live owner's journal or replace an unacknowledged request.
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&inbox)
            .context("a Pending requirements request already exists; resume it before submitting another")?;
        file.write_all(&serde_json::to_vec_pretty(&pending)?)?;
        file.sync_all()?;
    }
    let claim = RunClaim::resume(
        &Path::new(&binding.context.root).join(".newton/optimize/claim"), &journal.run_id,
    ).context("requirements request remains Pending; no update is active while a live or different owner holds the context")?;
    // Ownership serializes the read/modify/write with every running driver.
    *journal = serde_json::from_slice(&fs::read(directory.join("journal.json"))?)?;
    let mut binding: BoundOptimizationDefinition = serde_json::from_value(journal.binding.clone())?;
    let pending: RequirementsRevision = serde_json::from_slice(&fs::read(&inbox)?)?;
    let safe = matches!(
        journal.phase,
        Phase::Ready | Phase::CycleComplete | Phase::Finished
    );
    if pending.base_revision != Some(binding.requirements.revision) {
        let reason = format!(
            "Pending requirements request is stale; current revision is {}",
            binding.requirements.revision
        );
        record_rejection(&directory, &serde_json::to_value(&pending)?, &reason)?;
        fs::remove_file(inbox)?;
        if safe {
            claim.release()?;
        }
        anyhow::bail!(reason);
    }
    journal
        .revisions
        .retain(|r| r.status != RequirementsRevisionStatus::Pending);
    journal.revisions.push(pending.clone());
    if safe {
        let activation = acknowledge_revision(
            &binding.requirements,
            &pending,
            RevisionBoundary {
                affected_work_paused: true,
                prior_actions: vec![format!(
                    "{} cycles and {} workflow work dispatches recorded before activation",
                    journal.cycle, journal.work_count
                )],
            },
            &enforcement,
        )?;
        journal
            .revisions
            .retain(|r| r.status != RequirementsRevisionStatus::Pending);
        journal.revisions.push(activation.superseded);
        journal.revisions.push(activation.active.clone());
        binding.requirements = activation.active;
        journal.binding = serde_json::to_value(&binding)?;
        journal.outcome = None;
        if journal.phase == Phase::Finished {
            journal.phase = Phase::CycleComplete;
        }
    }
    newton_core::fs_util::atomic_write(
        &directory.join("journal.json"),
        &serde_json::to_vec_pretty(journal)?,
    )?;
    let store = newton_backend::SqliteBackendStore::new(
        &crate::cli::workspace_paths::state_backend_sqlite_url(state_dir),
    )
    .await
    .map_err(|e| anyhow!("open optimization store: {}", e.message))?;
    store
        .patch_optimize_run(
            &journal.run_id,
            PatchOptimizeRunBody {
                outcome_reason: Some(serde_json::to_value(&*journal)?),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| anyhow!("persist requirements revision: {}", e.message))?;
    if safe {
        fs::remove_file(inbox)?;
        claim.release()?;
        println!(
            "requirements revision {} is Active; retained results require current evidence",
            binding.requirements.revision
        );
    }
    // Drop retains Pending/unknown ownership; safe control releases before resume.
    Ok(safe)
}

fn record_rejection(directory: &Path, request: &serde_json::Value, reason: &str) -> Result<()> {
    let rejected = serde_json::json!({"status": "rejected", "request": request, "reason": reason});
    newton_core::fs_util::atomic_write(
        &directory.join(format!(
            "requirements-rejected-{}.json",
            uuid::Uuid::new_v4()
        )),
        &serde_json::to_vec_pretty(&rejected)?,
    )?;
    Ok(())
}
