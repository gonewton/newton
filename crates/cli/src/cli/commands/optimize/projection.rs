//! Optional terminal Run-status projection; tracker data never supplies work.

use super::lifecycle::{Journal, Lifecycle, Phase};
use newton_core::optimization::authorize_action;
use newton_projections::{
    FileProjectionStore, GithubProjectProjection, ProjectionKey, ProjectionPort, ProjectionStore,
    RunProjectionConfiguration, RunProjectionReport, RunProjectionService, RunProjectionSnapshot,
};
use newton_types::optimization::{ExecutionAction, ExecutionAuthority};
use newton_types::{ApiError, BackendStore, OptimizeRunDetail};
use std::{path::Path, sync::Arc};

pub(super) fn report(report: &RunProjectionReport) {
    if report.configured {
        if let Ok(rendered) = serde_json::to_string(report) {
            eprintln!("optimization projection: {rendered}");
        }
    }
}

/// Freeze optional local configuration before agents run. An absent file is true
/// no-tracker mode and creates no projection journal, adapter, or external write.
/// Configuration/permission failures remain separately visible projection errors.
pub(super) fn prepare(
    lifecycle: &mut Lifecycle,
    workspace: &Path,
    authority: &ExecutionAuthority,
) -> RunProjectionReport {
    if lifecycle.journal.projection_prepared {
        return lifecycle
            .journal
            .projection_report
            .clone()
            .unwrap_or_default();
    }
    let path = workspace.join(".newton/projections.json");
    let configuration = match std::fs::read_to_string(&path) {
        Ok(source) => RunProjectionConfiguration::parse(&source)
            .map(Some)
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "cannot read optional projection configuration: {error}"
        )),
    };
    let mut report = RunProjectionReport::default();
    match configuration {
        Ok(Some(configuration)) if !configuration.destinations.is_empty() => {
            report.configured = true;
            match authorize_github_projection(authority) {
                Ok(()) => lifecycle.journal.projection_configuration = Some(configuration),
                Err(error) => report.diagnostics.push(error.to_string()),
            }
        }
        Ok(_) => {}
        Err(error) => {
            report.configured = true;
            report.diagnostics.push(error);
        }
    }
    lifecycle.journal.projection_prepared = true;
    lifecycle.journal.projection_report = Some(report.clone());
    if let Err(error) = lifecycle.save() {
        // No external dispatch is authorized from an unpersisted configuration.
        lifecycle.journal.projection_configuration = None;
        report.diagnostics.push(format!(
            "projection configuration was not durably frozen: {error}"
        ));
        lifecycle.journal.projection_report = Some(report.clone());
    }
    report
}

/// Reflect only after the native outcome was durably persisted. Failure to deliver
/// does not change the optimizer outcome, acceptance, or next-work state.
pub(super) async fn reflect(
    lifecycle: &mut Lifecycle,
    workspace: &Path,
    state_dir: &Path,
    authority: &ExecutionAuthority,
) -> RunProjectionReport {
    if lifecycle.journal.projection_configuration.is_none() {
        return lifecycle
            .journal
            .projection_report
            .clone()
            .unwrap_or_default();
    }
    let port = Arc::new(GithubProjectProjection::with_existing_gh(workspace));
    reflect_with_port(lifecycle, state_dir, authority, port).await
}

async fn reflect_with_port(
    lifecycle: &mut Lifecycle,
    state_dir: &Path,
    authority: &ExecutionAuthority,
    port: Arc<dyn ProjectionPort>,
) -> RunProjectionReport {
    let Some(configuration) = lifecycle.journal.projection_configuration.clone() else {
        return lifecycle
            .journal
            .projection_report
            .clone()
            .unwrap_or_default();
    };
    let snapshot = lifecycle
        .store
        .get_optimize_run(&lifecycle.journal.run_id)
        .await;
    let mut report = reflect_snapshot(
        snapshot,
        configuration,
        lifecycle.journal.outcome.is_some(),
        state_dir,
        authority,
        port,
    )
    .await;
    lifecycle.journal.projection_report = Some(report.clone());
    if let Err(error) = save_projection_report(&lifecycle.journal.run_id, state_dir, &report) {
        report
            .diagnostics
            .push(format!("projection report persistence failed: {error}"));
        lifecycle.journal.projection_report = Some(report.clone());
    }
    report
}

/// Retry projection for a finished run without claiming optimization ownership,
/// modifying its journal/domain state, or reloading mutable source configuration.
/// The latest retry report is stored separately as `projection-report.json`.
pub(super) async fn retry_finished(
    journal: &Journal,
    workspace: &Path,
    state_dir: &Path,
    authority: &ExecutionAuthority,
) -> RunProjectionReport {
    if journal.projection_configuration.is_none() {
        return journal.projection_report.clone().unwrap_or_default();
    }
    let port = Arc::new(GithubProjectProjection::with_existing_gh(workspace));
    retry_finished_with_port(journal, state_dir, authority, port).await
}

async fn retry_finished_with_port(
    journal: &Journal,
    state_dir: &Path,
    authority: &ExecutionAuthority,
    port: Arc<dyn ProjectionPort>,
) -> RunProjectionReport {
    let Some(configuration) = journal.projection_configuration.clone() else {
        return journal.projection_report.clone().unwrap_or_default();
    };
    let mut report = RunProjectionReport {
        configured: true,
        ..Default::default()
    };
    if !matches!(journal.phase, Phase::Finished | Phase::Failed)
        || journal.outcome.is_none()
        || journal.run_id.is_empty()
        || !journal.run_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
    {
        report
            .diagnostics
            .push("projection retry requires a terminal Run journal with a valid identity".into());
        return report;
    }
    let database = crate::cli::workspace_paths::state_backend_sqlite(state_dir);
    if !database.is_file() {
        report.diagnostics.push("the existing optimization store is missing; projection retry did not create a replacement".into());
        return report;
    }
    match newton_backend::SqliteBackendStore::new(
        &crate::cli::workspace_paths::state_backend_sqlite_url(state_dir),
    )
    .await
    {
        Ok(store) => {
            report = reflect_snapshot(
                store.get_optimize_run(&journal.run_id).await,
                configuration,
                true,
                state_dir,
                authority,
                port,
            )
            .await;
        }
        Err(error) => report.diagnostics.push(format!(
            "cannot reopen optimization store for projection retry: {}",
            error.message
        )),
    }
    if let Err(error) = save_projection_report(&journal.run_id, state_dir, &report) {
        report.diagnostics.push(format!(
            "projection retry report persistence failed: {error}"
        ));
    }
    report
}

fn save_projection_report(
    run_id: &str,
    state_dir: &Path,
    report: &RunProjectionReport,
) -> Result<(), newton_projections::ProjectionError> {
    if run_id.is_empty()
        || !run_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
    {
        return Err(newton_projections::ProjectionError::Invalid(
            "invalid run identity for projection report".into(),
        ));
    }
    // This lock protects only delivery/report metadata, never Run ownership.
    let report_store = FileProjectionStore::new(state_dir.join("projections/reports"));
    let report_key = ProjectionKey {
        entity_kind: "optimize_run_report".into(),
        entity_id: run_id.into(),
        destination: "local_report".into(),
    };
    report_store.acquire(&report_key).and_then(|_lease| {
        let bytes = serde_json::to_vec_pretty(report)?;
        newton_core::fs_util::atomic_write(
            &state_dir
                .join("optimize")
                .join(run_id)
                .join("projection-report.json"),
            &bytes,
        )
        .map_err(newton_projections::ProjectionError::from)
    })
}

async fn reflect_snapshot(
    snapshot: Result<OptimizeRunDetail, ApiError>,
    configuration: RunProjectionConfiguration,
    terminal_outcome: bool,
    state_dir: &Path,
    authority: &ExecutionAuthority,
    port: Arc<dyn ProjectionPort>,
) -> RunProjectionReport {
    let mut report = RunProjectionReport {
        configured: true,
        ..Default::default()
    };
    match snapshot {
        Err(error) => report.diagnostics.push(format!(
            "cannot read durable Optimize Run for projection: {}",
            error.message
        )),
        Ok(snapshot) => {
            // This hook observes one terminal status per Cycle. Repeated terminal
            // hooks use the same revision, so retries are idempotent. A conflicting
            // terminal payload in the same cycle requires reconciliation.
            let revision = u64::try_from(snapshot.run.cycle)
                .ok()
                .and_then(|cycle| cycle.checked_add(1));
            if let Some(revision) =
                revision.filter(|_| snapshot.run.status != "running" && terminal_outcome)
            {
                let service = RunProjectionService::new(
                    snapshot.run.id.clone(),
                    configuration,
                    authorize_github_projection(authority).is_ok(),
                    FileProjectionStore::new(state_dir.join("projections/delivery")),
                    port,
                );
                match service {
                    Err(error) => report.diagnostics.push(error.to_string()),
                    Ok(service) => {
                        report = service
                            .reflect(&RunProjectionSnapshot {
                                run_id: snapshot.run.id,
                                revision,
                                status: snapshot.run.status,
                            })
                            .await;
                    }
                }
            } else {
                report.diagnostics.push(
                    "projection requires a durably recorded terminal Run outcome and valid cycle"
                        .into(),
                );
            }
        }
    }
    report
}

fn authorize_github_projection(
    authority: &ExecutionAuthority,
) -> Result<(), newton_core::optimization::OptimizationError> {
    // This adapter executes gh and makes a network write. A publication grant
    // cannot override a separate command or network prohibition.
    for action in [
        ExecutionAction::Command,
        ExecutionAction::Network,
        ExecutionAction::Publish,
    ] {
        authorize_action(authority, action)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
