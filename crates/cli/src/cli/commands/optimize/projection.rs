//! Optional terminal Run-status projection derived from authoritative JSON state.

use super::lifecycle::{Journal, Lifecycle, Phase};
use newton_core::optimization::authorize_action;
use newton_projections::{
    FileProjectionStore, GithubProjectProjection, ProjectionKey, ProjectionPort, ProjectionStore,
    RunProjectionConfiguration, RunProjectionReport, RunProjectionService, RunProjectionSnapshot,
};
use newton_types::optimization::{ExecutionAction, ExecutionAuthority};
use std::{path::Path, sync::Arc};

pub(super) fn report(report: &RunProjectionReport) {
    if report.configured {
        if let Ok(rendered) = serde_json::to_string(report) {
            eprintln!("optimization projection: {rendered}");
        }
    }
}

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
        lifecycle.journal.projection_configuration = None;
        report.diagnostics.push(format!(
            "projection configuration was not durably frozen: {error}"
        ));
        lifecycle.journal.projection_report = Some(report.clone());
    }
    report
}

pub(super) async fn reflect(
    lifecycle: &mut Lifecycle,
    workspace: &Path,
    state_dir: &Path,
    authority: &ExecutionAuthority,
) -> RunProjectionReport {
    let Some(configuration) = lifecycle.journal.projection_configuration.clone() else {
        return lifecycle
            .journal
            .projection_report
            .clone()
            .unwrap_or_default();
    };
    let port = Arc::new(GithubProjectProjection::with_existing_gh(workspace));
    let mut report = reflect_snapshot(
        &lifecycle.journal,
        configuration,
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

pub(super) async fn retry_finished(
    journal: &Journal,
    workspace: &Path,
    state_dir: &Path,
    authority: &ExecutionAuthority,
) -> RunProjectionReport {
    let Some(configuration) = journal.projection_configuration.clone() else {
        return journal.projection_report.clone().unwrap_or_default();
    };
    if !matches!(journal.phase, Phase::Finished | Phase::Failed)
        || journal.outcome.is_none()
        || journal.run_id.is_empty()
    {
        return RunProjectionReport {
            configured: true,
            diagnostics: vec![
                "projection retry requires a terminal Run checkpoint with a valid identity".into(),
            ],
            ..Default::default()
        };
    }
    let port = Arc::new(GithubProjectProjection::with_existing_gh(workspace));
    let mut report = reflect_snapshot(journal, configuration, state_dir, authority, port).await;
    if let Err(error) = save_projection_report(&journal.run_id, state_dir, &report) {
        report.diagnostics.push(format!(
            "projection retry report persistence failed: {error}"
        ));
    }
    report
}

async fn reflect_snapshot(
    journal: &Journal,
    configuration: RunProjectionConfiguration,
    state_dir: &Path,
    authority: &ExecutionAuthority,
    port: Arc<dyn ProjectionPort>,
) -> RunProjectionReport {
    let mut report = RunProjectionReport {
        configured: true,
        ..Default::default()
    };
    if journal.outcome.is_none() {
        report
            .diagnostics
            .push("projection requires a durably recorded terminal outcome".into());
        return report;
    }
    let service = RunProjectionService::new(
        journal.run_id.clone(),
        configuration,
        authorize_github_projection(authority).is_ok(),
        FileProjectionStore::new(state_dir.join("projections/delivery")),
        port,
    );
    match service {
        Err(error) => report.diagnostics.push(error.to_string()),
        Ok(service) => {
            let status = match journal.phase {
                Phase::Finished => "finished",
                Phase::Failed => "failed",
                _ => "running",
            };
            report = service
                .reflect(&RunProjectionSnapshot {
                    run_id: journal.run_id.clone(),
                    revision: journal.cycle.saturating_add(1),
                    status: status.into(),
                })
                .await;
        }
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

fn authorize_github_projection(
    authority: &ExecutionAuthority,
) -> Result<(), newton_core::optimization::OptimizationError> {
    for action in [
        ExecutionAction::Command,
        ExecutionAction::Network,
        ExecutionAction::Publish,
    ] {
        authorize_action(authority, action)?;
    }
    Ok(())
}
