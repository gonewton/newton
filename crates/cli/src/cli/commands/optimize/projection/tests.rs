use super::super::lifecycle::Phase;
use super::*;
use async_trait::async_trait;
use newton_projections::{
    DeliveryOutcome, GhCommand, ProjectionBinding, ProjectionError, RunProjectionDestination,
};
use newton_types::{BackendStore, BroadcastEvent};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

#[derive(Default)]
struct FakeGh {
    offline: AtomicBool,
    calls: Mutex<Vec<Vec<String>>>,
    remote: Mutex<BTreeMap<String, String>>,
}

#[async_trait]
impl GhCommand for FakeGh {
    async fn run(&self, args: &[String]) -> Result<(), ProjectionError> {
        self.calls.lock().unwrap().push(args.to_vec());
        if self.offline.load(Ordering::SeqCst) {
            return Err(ProjectionError::Delivery("GitHub unavailable".into()));
        }
        assert_eq!(&args[..2], &["project", "item-edit"]);
        self.remote
            .lock()
            .unwrap()
            .insert(args[5].clone(), args[9].clone());
        Ok(())
    }
}

fn authority() -> ExecutionAuthority {
    ExecutionAuthority {
        allowed_actions: [
            ExecutionAction::Publish,
            ExecutionAction::Network,
            ExecutionAction::Command,
        ]
        .into(),
    }
}

fn configuration(item: &str) -> RunProjectionConfiguration {
    RunProjectionConfiguration {
        schema_version: 1,
        destinations: vec![RunProjectionDestination {
            name: "delivery-board".into(),
            binding: ProjectionBinding::GithubProjectItem {
                project_id: "project".into(),
                item_id: item.into(),
                field_id: "status".into(),
                status_options: [
                    ("converged".into(), "done".into()),
                    ("resource_limit".into(), "stopped".into()),
                    ("failed".into(), "blocked".into()),
                ]
                .into(),
            },
        }],
        max_delivery_attempts: 8,
        timeout_seconds: 5,
    }
}

async fn lifecycle() -> (tempfile::TempDir, Lifecycle, Arc<dyn BackendStore>) {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join(".newton/state");
    std::fs::create_dir_all(&state).unwrap();
    let store: Arc<dyn BackendStore> = Arc::new(
        newton_backend::SqliteBackendStore::new(&format!(
            "sqlite:{}?mode=rwc",
            state.join("backend.sqlite").display()
        ))
        .await
        .unwrap(),
    );
    let (events, _) = tokio::sync::broadcast::channel::<BroadcastEvent>(16);
    let lifecycle = Lifecycle::start(
        store.clone(),
        events,
        &state,
        directory.path(),
        "run-one",
        "project",
        2,
        vec![],
        json!({}),
    )
    .await
    .unwrap();
    (directory, lifecycle, store)
}

fn write_configuration(workspace: &Path, config: &RunProjectionConfiguration) {
    std::fs::write(
        workspace.join(".newton/projections.json"),
        serde_json::to_vec_pretty(config).unwrap(),
    )
    .unwrap();
}

async fn finish(lifecycle: &mut Lifecycle, status: &str) {
    lifecycle.journal.cycle = 1;
    lifecycle
        .finish(
            status,
            json!({"accepted_result": "candidate-one", "completed": status == "converged"}),
            true,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn absent_configuration_needs_no_tracker_and_stays_absent_on_resume() {
    let (directory, mut lifecycle, store) = lifecycle().await;
    let state = directory.path().join(".newton/state");
    let prepared = prepare(
        &mut lifecycle,
        directory.path(),
        &ExecutionAuthority::default(),
    );
    assert!(!prepared.configured);
    assert!(!state.join("projections").exists());
    lifecycle.phase(Phase::CycleComplete).await.unwrap();
    let saved = serde_json::from_slice(
        &std::fs::read(state.join("optimize/run-one/journal.json")).unwrap(),
    )
    .unwrap();
    let (events, _) = tokio::sync::broadcast::channel(16);
    drop(lifecycle);
    write_configuration(directory.path(), &configuration("late-source-edit"));
    let mut resumed = Lifecycle::resume(saved, store, events, &state, directory.path())
        .await
        .unwrap();
    assert!(!prepare(&mut resumed, directory.path(), &authority()).configured);
    finish(&mut resumed, "converged").await;
    let report = reflect(&mut resumed, directory.path(), &state, &authority()).await;
    assert!(!report.configured);
    assert!(
        !retry_finished(&resumed.journal, directory.path(), &state, &authority())
            .await
            .configured
    );
    assert!(!state.join("projections").exists());
}

#[tokio::test]
async fn frozen_configuration_survives_resume_and_later_source_changes() {
    let (directory, mut lifecycle, store) = lifecycle().await;
    let state = directory.path().join(".newton/state");
    write_configuration(directory.path(), &configuration("approved-item"));
    assert!(prepare(&mut lifecycle, directory.path(), &authority())
        .diagnostics
        .is_empty());
    lifecycle.phase(Phase::CycleComplete).await.unwrap();
    let saved = serde_json::from_slice(
        &std::fs::read(state.join("optimize/run-one/journal.json")).unwrap(),
    )
    .unwrap();
    let (events, _) = tokio::sync::broadcast::channel(16);
    drop(lifecycle);
    write_configuration(directory.path(), &configuration("agent-edited-item"));
    let mut resumed = Lifecycle::resume(saved, store, events, &state, directory.path())
        .await
        .unwrap();
    prepare(&mut resumed, directory.path(), &authority());
    finish(&mut resumed, "converged").await;
    let fake = Arc::new(FakeGh::default());
    let adapter = Arc::new(GithubProjectProjection::with_command(fake.clone()));
    let report = reflect_with_port(&mut resumed, &state, &authority(), adapter).await;
    assert!(matches!(
        report.targets[0].delivery,
        Some(DeliveryOutcome::Delivered { .. })
    ));
    assert!(fake.remote.lock().unwrap().contains_key("approved-item"));
    assert!(!fake
        .remote
        .lock()
        .unwrap()
        .contains_key("agent-edited-item"));
}

#[tokio::test]
async fn tracker_outage_and_repeated_delivery_do_not_mutate_native_run_or_cycles() {
    let (directory, mut lifecycle, store) = lifecycle().await;
    let state = directory.path().join(".newton/state");
    write_configuration(directory.path(), &configuration("approved-item"));
    prepare(&mut lifecycle, directory.path(), &authority());
    finish(&mut lifecycle, "converged").await;
    let before = serde_json::to_value(store.get_optimize_run("run-one").await.unwrap()).unwrap();
    let journal_before = std::fs::read(state.join("optimize/run-one/journal.json")).unwrap();
    let cycles =
        serde_json::to_value(store.list_optimize_cycles("run-one").await.unwrap()).unwrap();
    let fake = Arc::new(FakeGh::default());
    fake.offline.store(true, Ordering::SeqCst);
    let adapter = Arc::new(GithubProjectProjection::with_command(fake.clone()));
    let failed = reflect_with_port(&mut lifecycle, &state, &authority(), adapter.clone()).await;
    assert!(matches!(
        failed.targets[0].delivery,
        Some(DeliveryOutcome::Deferred { .. })
    ));
    fake.offline.store(false, Ordering::SeqCst);
    let recovered = reflect_with_port(&mut lifecycle, &state, &authority(), adapter.clone()).await;
    assert!(matches!(
        recovered.targets[0].delivery,
        Some(DeliveryOutcome::Delivered { .. })
    ));
    let duplicate = reflect_with_port(&mut lifecycle, &state, &authority(), adapter).await;
    assert!(matches!(
        duplicate.targets[0].delivery,
        Some(DeliveryOutcome::AlreadyDelivered { .. })
    ));
    assert_eq!(fake.calls.lock().unwrap().len(), 2);
    assert_eq!(
        serde_json::to_value(store.get_optimize_run("run-one").await.unwrap()).unwrap(),
        before
    );
    assert_eq!(
        serde_json::to_value(store.list_optimize_cycles("run-one").await.unwrap()).unwrap(),
        cycles
    );
    let persisted: serde_json::Value = serde_json::from_slice(
        &std::fs::read(state.join("optimize/run-one/projection-report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        persisted["targets"][0]["delivery"]["kind"],
        "already_delivered"
    );
    assert_eq!(
        std::fs::read(state.join("optimize/run-one/journal.json")).unwrap(),
        journal_before
    );
}

#[tokio::test]
async fn resource_limit_projects_stopped_without_changing_the_internal_outcome() {
    let (directory, mut lifecycle, store) = lifecycle().await;
    let state = directory.path().join(".newton/state");
    write_configuration(directory.path(), &configuration("approved-item"));
    prepare(&mut lifecycle, directory.path(), &authority());
    finish(&mut lifecycle, "resource_limit").await;
    let before = serde_json::to_value(store.get_optimize_run("run-one").await.unwrap()).unwrap();
    let fake = Arc::new(FakeGh::default());
    let adapter = Arc::new(GithubProjectProjection::with_command(fake.clone()));
    let report = reflect_with_port(&mut lifecycle, &state, &authority(), adapter).await;
    assert!(matches!(
        report.targets[0].delivery,
        Some(DeliveryOutcome::Delivered { .. })
    ));
    assert_eq!(fake.remote.lock().unwrap()["approved-item"], "stopped");
    assert_eq!(before["status"], "resource_limit");
    assert_eq!(before["outcomeReason"]["completed"], false);
    assert_eq!(
        serde_json::to_value(store.get_optimize_run("run-one").await.unwrap()).unwrap(),
        before
    );
}

#[tokio::test]
async fn external_board_edits_never_select_work_or_change_internal_failure() {
    let (directory, mut lifecycle, store) = lifecycle().await;
    let state = directory.path().join(".newton/state");
    write_configuration(directory.path(), &configuration("approved-item"));
    prepare(&mut lifecycle, directory.path(), &authority());
    finish(&mut lifecycle, "failed").await;
    let before = serde_json::to_value(store.get_optimize_run("run-one").await.unwrap()).unwrap();
    let fake = Arc::new(FakeGh::default());
    fake.remote
        .lock()
        .unwrap()
        .insert("approved-item".into(), "externally-approved".into());
    let adapter = Arc::new(GithubProjectProjection::with_command(fake.clone()));
    reflect_with_port(&mut lifecycle, &state, &authority(), adapter).await;
    assert_eq!(fake.remote.lock().unwrap()["approved-item"], "blocked");
    assert_eq!(
        serde_json::to_value(store.get_optimize_run("run-one").await.unwrap()).unwrap(),
        before
    );
    assert!(fake
        .calls
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .all(|arg| !["view", "list", "search", "create"].contains(&arg.as_str())));
}

#[tokio::test]
async fn projection_permission_failure_is_visible_without_failing_optimization() {
    let (directory, mut lifecycle, store) = lifecycle().await;
    let state = directory.path().join(".newton/state");
    write_configuration(directory.path(), &configuration("approved-item"));
    let preparation = prepare(
        &mut lifecycle,
        directory.path(),
        &ExecutionAuthority::default(),
    );
    assert!(preparation.configured);
    assert!(!preparation.diagnostics.is_empty());
    assert!(lifecycle.journal.projection_configuration.is_none());
    finish(&mut lifecycle, "converged").await;
    let report = reflect(
        &mut lifecycle,
        directory.path(),
        &state,
        &ExecutionAuthority::default(),
    )
    .await;
    assert!(!report.diagnostics.is_empty());
    assert_eq!(
        store.get_optimize_run("run-one").await.unwrap().run.status,
        "converged"
    );
    assert!(!state.join("projections").exists());
}

#[test]
fn publication_cannot_bypass_command_or_network_prohibitions() {
    for denied in [
        ExecutionAction::Command,
        ExecutionAction::Network,
        ExecutionAction::Publish,
    ] {
        let mut restricted = authority();
        restricted.allowed_actions.remove(&denied);
        assert!(authorize_github_projection(&restricted).is_err());
    }
    authorize_github_projection(&authority()).unwrap();
}

#[tokio::test]
async fn finished_retry_reopens_same_store_and_only_persists_projection_metadata() {
    let (directory, mut lifecycle, store) = lifecycle().await;
    let state = directory.path().join(".newton/state");
    write_configuration(directory.path(), &configuration("frozen-item"));
    prepare(&mut lifecycle, directory.path(), &authority());
    finish(&mut lifecycle, "converged").await;
    let fake = Arc::new(FakeGh::default());
    fake.offline.store(true, Ordering::SeqCst);
    let adapter = Arc::new(GithubProjectProjection::with_command(fake.clone()));
    let deferred = reflect_with_port(&mut lifecycle, &state, &authority(), adapter.clone()).await;
    assert!(matches!(
        deferred.targets[0].delivery,
        Some(DeliveryOutcome::Deferred { .. })
    ));
    let journal = lifecycle.journal.clone();
    drop(lifecycle);
    let journal_path = state.join("optimize/run-one/journal.json");
    let journal_before = std::fs::read(&journal_path).unwrap();
    let run_before =
        serde_json::to_value(store.get_optimize_run("run-one").await.unwrap()).unwrap();
    let claim_path = directory.path().join(".newton/optimize/claim");
    let unrelated_claim =
        super::super::ownership::RunClaim::acquire(&claim_path, "another-run").unwrap();
    let owner_before = std::fs::read(claim_path.join("owner.json")).unwrap();
    write_configuration(directory.path(), &configuration("changed-source-item"));
    fake.offline.store(false, Ordering::SeqCst);
    let recovered = retry_finished_with_port(&journal, &state, &authority(), adapter.clone()).await;
    assert!(matches!(
        recovered.targets[0].delivery,
        Some(DeliveryOutcome::Delivered { .. })
    ));
    let duplicate = retry_finished_with_port(&journal, &state, &authority(), adapter).await;
    assert!(matches!(
        duplicate.targets[0].delivery,
        Some(DeliveryOutcome::AlreadyDelivered { .. })
    ));
    assert_eq!(fake.calls.lock().unwrap().len(), 2);
    assert!(fake.remote.lock().unwrap().contains_key("frozen-item"));
    assert!(!fake
        .remote
        .lock()
        .unwrap()
        .contains_key("changed-source-item"));
    assert_eq!(std::fs::read(&journal_path).unwrap(), journal_before);
    assert_eq!(
        std::fs::read(claim_path.join("owner.json")).unwrap(),
        owner_before
    );
    assert_eq!(
        serde_json::to_value(store.get_optimize_run("run-one").await.unwrap()).unwrap(),
        run_before
    );
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(state.join("optimize/run-one/projection-report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        report["targets"][0]["delivery"]["kind"],
        "already_delivered"
    );
    unrelated_claim.release().unwrap();
}
