//! Local, serialized requirements control. An inbox is not an active revision.

use super::{
    lifecycle::{Journal, Phase},
    ownership::RunClaim,
};
use anyhow::{Context, Result};
use newton_core::optimization::{
    acknowledge_revision, propose_revision, EnforcementCapabilities, RequirementsUpdateAuthority,
    RevisionActivation, RevisionBoundary,
};
use newton_types::optimization::*;
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
        if let Err(error) = validate_pending(journal, &binding, &pending) {
            record_rejection(
                &directory,
                &serde_json::to_value(&pending)?,
                &error.to_string(),
            )?;
            return Err(error);
        }
        // Publish only complete JSON. create_new followed by write exposes a
        // partial request to the running owner's safe-boundary reader.
        let mut file = tempfile::NamedTempFile::new_in(&directory)?;
        file.write_all(&serde_json::to_vec_pretty(&pending)?)?;
        file.as_file().sync_all()?;
        file.persist_noclobber(&inbox).context(
            "a Pending requirements request already exists; resume it before submitting another",
        )?;
    }
    let claim = RunClaim::resume(
        &Path::new(&binding.context.root).join(".newton/optimize/claim"), &journal.run_id,
    ).context("requirements request remains Pending; no update is active while a live or different owner holds the context")?;
    // Ownership serializes the read/modify/write with every running driver.
    *journal = serde_json::from_slice(&fs::read(directory.join("current.json"))?)?;
    let mut binding: BoundOptimizationDefinition = serde_json::from_value(journal.binding.clone())?;
    let pending: RequirementsRevision = serde_json::from_slice(&fs::read(&inbox)?)?;
    let safe = !journal.requires_reconciliation
        && matches!(
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
    if let Err(error) = validate_pending(journal, &binding, &pending) {
        record_rejection(
            &directory,
            &serde_json::to_value(&pending)?,
            &error.to_string(),
        )?;
        fs::remove_file(inbox)?;
        if safe {
            claim.release()?;
        }
        return Err(error);
    }
    journal
        .revisions
        .retain(|r| r.status != RequirementsRevisionStatus::Pending);
    journal.revisions.push(pending.clone());
    if safe {
        let activation = acknowledge_pending(journal, &binding, &pending)?;
        journal
            .revisions
            .retain(|r| r.status != RequirementsRevisionStatus::Pending);
        journal.revisions.push(activation.superseded);
        journal.revisions.push(activation.active.clone());
        if let Some(prior) = journal.accepted.take() {
            journal.retained = Some(prior.clone());
            journal.accepted_history.push(prior);
        }
        binding.requirements = activation.active;
        journal.binding = serde_json::to_value(&binding)?;
        journal.outcome = None;
        if journal.phase == Phase::Finished {
            journal.phase = Phase::CycleComplete;
        }
    }
    newton_core::fs_util::atomic_write(
        &directory.join("current.json"),
        &serde_json::to_vec_pretty(journal)?,
    )?;
    if safe && journal.phase == Phase::CycleComplete {
        remove_if_present(&directory.join("outcome.json"))?;
        remove_if_present(&directory.join("report.json"))?;
    }
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

fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn validate_pending(
    journal: &Journal,
    binding: &BoundOptimizationDefinition,
    pending: &RequirementsRevision,
) -> Result<()> {
    super::workflow::evaluator_reference(&pending.requirements)?;
    // Revalidate the inbox against current CAS, immutable authority ceiling and
    // capabilities. The serialized request cannot grant authority to itself.
    let authority = RequirementsUpdateAuthority {
        actor: pending.requested_by.clone(),
        may_update_requirements: true,
        may_update_evaluators: true,
        may_update_execution_restrictions: true,
    };
    let expected = propose_revision(
        &binding.requirements,
        &binding.authority_ceiling,
        RequirementsUpdate {
            base_revision: pending
                .base_revision
                .context("Pending request lacks base revision")?,
            requirements: pending.requirements.clone(),
            parameter_overrides: pending.parameters.clone(),
        },
        &authority,
        &EnforcementCapabilities::default(),
    )?;
    if expected.authority != pending.authority
        || expected.parameters != pending.parameters
        || expected.revision != pending.revision
        || pending.status != RequirementsRevisionStatus::Pending
    {
        anyhow::bail!("Pending request differs from authorized parameter/authority binding");
    }
    let mut proposed = binding.clone();
    proposed.requirements = pending.clone();
    journal
        .definition_snapshot
        .as_ref()
        .context("run has no immutable definition snapshot")?
        .verify(&proposed, &journal.definition_root)?;
    Ok(())
}

fn acknowledge_pending(
    journal: &Journal,
    binding: &BoundOptimizationDefinition,
    pending: &RequirementsRevision,
) -> Result<RevisionActivation> {
    Ok(acknowledge_revision(
        &binding.requirements,
        pending,
        RevisionBoundary {
            affected_work_paused: true,
            prior_actions: vec![format!(
                "completed workflow work before activation: {} cycles, {} work dispatches, {} evaluations; prior actions are not undone",
                journal.cycle, journal.work_count, journal.evaluation_count
            )],
        },
        &EnforcementCapabilities::default(),
    )?)
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
