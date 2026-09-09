use async_trait::async_trait;
use newton_projections::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

#[derive(Default)]
struct FakePort {
    calls: AtomicUsize,
    offline: AtomicBool,
}

#[async_trait]
impl ProjectionPort for FakePort {
    async fn reflect(
        &self,
        _binding: &ProjectionBinding,
        _update: &ProjectionUpdate,
    ) -> Result<ProjectionReceipt, ProjectionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.offline.load(Ordering::SeqCst) {
            return Err(ProjectionError::Delivery("tracker offline".into()));
        }
        Ok(ProjectionReceipt {
            external_ref: Some("existing-item".into()),
        })
    }
}

fn config(count: usize) -> RunProjectionConfiguration {
    RunProjectionConfiguration {
        schema_version: 1,
        destinations: (0..count)
            .map(|index| RunProjectionDestination {
                name: format!("board-{index}"),
                binding: ProjectionBinding::GithubProjectItem {
                    project_id: "project".into(),
                    item_id: format!("item-{index}"),
                    field_id: "status".into(),
                    status_options: [("converged".into(), "done".into())].into(),
                },
            })
            .collect(),
        max_delivery_attempts: 8,
        timeout_seconds: 5,
    }
}

fn snapshot() -> RunProjectionSnapshot {
    RunProjectionSnapshot {
        run_id: "run-1".into(),
        revision: 1,
        status: "converged".into(),
    }
}

#[test]
fn configuration_is_strict_bounded_and_explicit() {
    let source = serde_json::to_string(&config(1)).unwrap();
    assert_eq!(
        RunProjectionConfiguration::parse(&source).unwrap(),
        config(1)
    );
    let mut value = serde_json::to_value(config(1)).unwrap();
    value["execute"] = serde_json::json!("arbitrary code");
    assert!(RunProjectionConfiguration::parse(&value.to_string()).is_err());
    for count in [0, 65] {
        let mut invalid = config(1);
        invalid.max_delivery_attempts = count;
        assert!(invalid.validate().is_err());
    }
    let mut invalid = config(1);
    invalid.timeout_seconds = 0;
    assert!(invalid.validate().is_err());
    invalid = config(2);
    invalid.destinations[1].name = invalid.destinations[0].name.clone();
    assert!(invalid.validate().is_err());
}

#[tokio::test]
async fn empty_configuration_has_no_tracker_or_projection_journal() {
    let directory = tempfile::tempdir().unwrap();
    let journal = directory.path().join("absent");
    let port = Arc::new(FakePort::default());
    let service = RunProjectionService::new(
        "run-1",
        config(0),
        false,
        FileProjectionStore::new(&journal),
        port.clone(),
    )
    .unwrap();
    let report = service.reflect(&snapshot()).await;
    assert!(!report.configured);
    assert!(report.targets.is_empty());
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    assert!(!journal.exists());
}

#[tokio::test]
async fn outage_retry_and_duplicate_snapshot_remain_durable_and_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let port = Arc::new(FakePort::default());
    port.offline.store(true, Ordering::SeqCst);
    let service = RunProjectionService::new(
        "run-1",
        config(1),
        true,
        FileProjectionStore::new(directory.path()),
        port.clone(),
    )
    .unwrap();
    let deferred = service.reflect(&snapshot()).await;
    assert!(matches!(
        deferred.targets[0].delivery,
        Some(DeliveryOutcome::Deferred { .. })
    ));
    drop(service);
    port.offline.store(false, Ordering::SeqCst);
    let restarted = RunProjectionService::new(
        "run-1",
        config(1),
        true,
        FileProjectionStore::new(directory.path()),
        port.clone(),
    )
    .unwrap();
    assert!(matches!(
        restarted.retry_pending().await.targets[0].delivery,
        Some(DeliveryOutcome::Delivered { .. })
    ));
    assert!(matches!(
        restarted.reflect(&snapshot()).await.targets[0].delivery,
        Some(DeliveryOutcome::AlreadyDelivered { .. })
    ));
    assert_eq!(port.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn bounded_delivery_queues_remaining_targets_and_retry_makes_progress() {
    let directory = tempfile::tempdir().unwrap();
    let port = Arc::new(FakePort::default());
    let mut limited = config(2);
    limited.max_delivery_attempts = 1;
    let service = RunProjectionService::new(
        "run-1",
        limited,
        true,
        FileProjectionStore::new(directory.path()),
        port.clone(),
    )
    .unwrap();
    let first = service.reflect(&snapshot()).await;
    assert!(matches!(
        first.targets[0].delivery,
        Some(DeliveryOutcome::Delivered { .. })
    ));
    assert!(matches!(
        first.targets[1].delivery,
        Some(DeliveryOutcome::Deferred { .. })
    ));
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    let retry = service.retry_pending().await;
    assert!(matches!(
        retry.targets[0].delivery,
        Some(DeliveryOutcome::Idle)
    ));
    assert!(matches!(
        retry.targets[1].delivery,
        Some(DeliveryOutcome::Delivered { .. })
    ));
    assert_eq!(port.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn unauthorized_or_cross_run_snapshots_cannot_write() {
    let directory = tempfile::tempdir().unwrap();
    let port = Arc::new(FakePort::default());
    assert!(RunProjectionService::new(
        "run-1",
        config(1),
        false,
        FileProjectionStore::new(directory.path()),
        port.clone()
    )
    .is_err());
    let service = RunProjectionService::new(
        "run-1",
        config(1),
        true,
        FileProjectionStore::new(directory.path()),
        port.clone(),
    )
    .unwrap();
    let mut other = snapshot();
    other.run_id = "run-2".into();
    assert!(!service.reflect(&other).await.diagnostics.is_empty());
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
}

struct HangingPort;

#[async_trait]
impl ProjectionPort for HangingPort {
    async fn reflect(
        &self,
        _binding: &ProjectionBinding,
        _update: &ProjectionUpdate,
    ) -> Result<ProjectionReceipt, ProjectionError> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn timeout_is_durably_deferred_not_delivery_or_optimization_success() {
    let directory = tempfile::tempdir().unwrap();
    let mut limited = config(1);
    limited.timeout_seconds = 1;
    let service = RunProjectionService::new(
        "run-1",
        limited,
        true,
        FileProjectionStore::new(directory.path()),
        Arc::new(HangingPort),
    )
    .unwrap();
    let report = service.reflect(&snapshot()).await;
    assert!(
        matches!(&report.targets[0].delivery, Some(DeliveryOutcome::Deferred { error, .. }) if error.contains("timed out"))
    );
    let dispatcher = ProjectionDispatcher::new(FileProjectionStore::new(directory.path()));
    let record = dispatcher
        .record(&ProjectionKey {
            entity_kind: "optimize_run".into(),
            entity_id: "run-1".into(),
            destination: "board-0".into(),
        })
        .unwrap()
        .unwrap();
    assert!(record.pending.is_some());
    assert!(record.delivered.is_none());
    assert!(record.last_error.unwrap().contains("timed out"));
}
