use async_trait::async_trait;
use newton_projections::*;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

fn key() -> ProjectionKey {
    ProjectionKey {
        entity_kind: "change_request".into(),
        entity_id: "cr-123".into(),
        destination: "engineering-board".into(),
    }
}

fn binding() -> ProjectionBinding {
    ProjectionBinding::GithubProjectItem {
        project_id: "project-1".into(),
        item_id: "existing-item-7".into(),
        field_id: "status-field".into(),
        status_options: [
            ("proposed".into(), "option-todo".into()),
            ("accepted".into(), "option-done".into()),
        ]
        .into(),
    }
}

fn update(revision: u64, status: &str) -> ProjectionUpdate {
    ProjectionUpdate {
        revision,
        derived_status: status.into(),
    }
}

#[derive(Default)]
struct FakeGh {
    calls: Mutex<Vec<Vec<String>>>,
    remote: Mutex<BTreeMap<String, String>>,
    fail_before: AtomicBool,
    fail_after: AtomicBool,
}

#[async_trait]
impl GhCommand for FakeGh {
    async fn run(&self, args: &[String]) -> Result<(), ProjectionError> {
        self.calls.lock().unwrap().push(args.to_vec());
        if self.fail_before.swap(false, Ordering::SeqCst) {
            return Err(ProjectionError::Delivery("tracker offline".into()));
        }
        assert_eq!(&args[..2], &["project", "item-edit"]);
        assert_eq!(args.len(), 10);
        assert_eq!(args[4], "--id");
        assert_eq!(args[8], "--single-select-option-id");
        self.remote
            .lock()
            .unwrap()
            .insert(args[5].clone(), args[9].clone());
        if self.fail_after.swap(false, Ordering::SeqCst) {
            return Err(ProjectionError::Delivery(
                "response lost after assignment".into(),
            ));
        }
        Ok(())
    }
}

#[tokio::test]
async fn no_tracker_requires_no_transport_credentials_or_journal() {
    let receipt = NoTracker
        .reflect(&ProjectionBinding::None, &update(1, "accepted"))
        .await
        .unwrap();
    assert_eq!(receipt.external_ref, None);
    assert!(NoTracker
        .reflect(&binding(), &update(1, "accepted"))
        .await
        .is_err());
}

#[tokio::test]
async fn repeated_delivery_is_local_and_binding_cannot_be_silently_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let dispatcher = ProjectionDispatcher::new(FileProjectionStore::new(directory.path()));
    dispatcher.bind(&key(), binding()).unwrap();
    dispatcher.bind(&key(), binding()).unwrap();
    assert!(matches!(
        dispatcher.bind(&key(), ProjectionBinding::None),
        Err(ProjectionError::BindingConflict)
    ));
    let fake = Arc::new(FakeGh::default());
    let adapter = GithubProjectProjection::with_command(fake.clone());
    assert!(matches!(
        dispatcher
            .deliver(&key(), update(1, "proposed"), &adapter)
            .await
            .unwrap(),
        DeliveryOutcome::Delivered { .. }
    ));
    assert_eq!(
        dispatcher
            .deliver(&key(), update(1, "proposed"), &adapter)
            .await
            .unwrap(),
        DeliveryOutcome::AlreadyDelivered { revision: 1 }
    );
    assert_eq!(fake.calls.lock().unwrap().len(), 1);
    let record = dispatcher.record(&key()).unwrap().unwrap();
    assert_eq!(record.delivered, Some(update(1, "proposed")));
    assert!(record.pending.is_none());
    assert_eq!(
        dispatcher.retry_pending(&key(), &adapter).await.unwrap(),
        DeliveryOutcome::Idle
    );
}

#[tokio::test]
async fn tracker_outage_is_deferred_and_retried_after_process_restart() {
    let directory = tempfile::tempdir().unwrap();
    let dispatcher = ProjectionDispatcher::new(FileProjectionStore::new(directory.path()));
    dispatcher.bind(&key(), binding()).unwrap();
    let fake = Arc::new(FakeGh::default());
    fake.fail_before.store(true, Ordering::SeqCst);
    let adapter = GithubProjectProjection::with_command(fake.clone());
    let result = dispatcher
        .deliver(&key(), update(2, "accepted"), &adapter)
        .await
        .unwrap();
    assert!(matches!(
        result,
        DeliveryOutcome::Deferred { revision: 2, .. }
    ));
    let record = dispatcher.record(&key()).unwrap().unwrap();
    assert_eq!(record.pending, Some(update(2, "accepted")));
    assert!(record.delivered.is_none());
    assert!(record.last_error.is_some());
    assert!(fake.remote.lock().unwrap().is_empty());
    drop(dispatcher);
    let restarted = ProjectionDispatcher::new(FileProjectionStore::new(directory.path()));
    assert!(matches!(
        restarted.retry_pending(&key(), &adapter).await.unwrap(),
        DeliveryOutcome::Delivered { revision: 2, .. }
    ));
    assert_eq!(fake.remote.lock().unwrap().len(), 1);
    assert_eq!(fake.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn ambiguous_external_response_retries_same_assignment_without_creation_or_search() {
    let directory = tempfile::tempdir().unwrap();
    let dispatcher = ProjectionDispatcher::new(FileProjectionStore::new(directory.path()));
    dispatcher.bind(&key(), binding()).unwrap();
    let fake = Arc::new(FakeGh::default());
    fake.fail_after.store(true, Ordering::SeqCst);
    let adapter = GithubProjectProjection::with_command(fake.clone());
    assert!(matches!(
        dispatcher
            .deliver(&key(), update(4, "accepted"), &adapter)
            .await
            .unwrap(),
        DeliveryOutcome::Deferred { .. }
    ));
    assert_eq!(
        fake.remote.lock().unwrap()["existing-item-7"],
        "option-done"
    );
    dispatcher.retry_pending(&key(), &adapter).await.unwrap();
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0], calls[1]);
    assert!(calls
        .iter()
        .flatten()
        .all(|argument| !["create", "search", "list", "view"].contains(&argument.as_str())));
    assert_eq!(fake.remote.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn stale_revisions_and_external_board_edits_cannot_decide_internal_status() {
    let directory = tempfile::tempdir().unwrap();
    let dispatcher = ProjectionDispatcher::new(FileProjectionStore::new(directory.path()));
    dispatcher.bind(&key(), binding()).unwrap();
    let fake = Arc::new(FakeGh::default());
    let adapter = GithubProjectProjection::with_command(fake.clone());
    dispatcher
        .deliver(&key(), update(5, "accepted"), &adapter)
        .await
        .unwrap();
    fake.remote
        .lock()
        .unwrap()
        .insert("existing-item-7".into(), "human-board-edit".into());
    assert_eq!(
        dispatcher
            .deliver(&key(), update(4, "proposed"), &adapter)
            .await
            .unwrap(),
        DeliveryOutcome::Superseded {
            current_revision: 5
        }
    );
    assert!(matches!(
        dispatcher
            .deliver(&key(), update(5, "proposed"), &adapter)
            .await,
        Err(ProjectionError::RevisionConflict(5))
    ));
    assert_eq!(
        dispatcher.record(&key()).unwrap().unwrap().delivered,
        Some(update(5, "accepted"))
    );
    dispatcher
        .deliver(&key(), update(6, "accepted"), &adapter)
        .await
        .unwrap();
    assert_eq!(
        fake.remote.lock().unwrap()["existing-item-7"],
        "option-done"
    );
    assert_eq!(fake.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn newer_internal_revision_supersedes_failed_pending_delivery() {
    let directory = tempfile::tempdir().unwrap();
    let dispatcher = ProjectionDispatcher::new(FileProjectionStore::new(directory.path()));
    dispatcher.bind(&key(), binding()).unwrap();
    let fake = Arc::new(FakeGh::default());
    fake.fail_before.store(true, Ordering::SeqCst);
    let adapter = GithubProjectProjection::with_command(fake.clone());
    dispatcher
        .deliver(&key(), update(2, "proposed"), &adapter)
        .await
        .unwrap();
    assert_eq!(
        dispatcher
            .deliver(&key(), update(1, "proposed"), &adapter)
            .await
            .unwrap(),
        DeliveryOutcome::Superseded {
            current_revision: 2
        }
    );
    dispatcher
        .deliver(&key(), update(3, "accepted"), &adapter)
        .await
        .unwrap();
    assert_eq!(
        dispatcher.record(&key()).unwrap().unwrap().delivered,
        Some(update(3, "accepted"))
    );
}

#[test]
fn cross_instance_owner_lock_and_hashed_identity_prevent_races_and_path_injection() {
    let directory = tempfile::tempdir().unwrap();
    let store = FileProjectionStore::new(directory.path());
    let mut malicious = key();
    malicious.entity_id = "../../outside".into();
    let lease = store.acquire(&malicious).unwrap();
    let second = FileProjectionStore::new(directory.path());
    assert!(matches!(
        second.acquire(&malicious),
        Err(ProjectionError::Busy)
    ));
    assert!(lease
        .save(&ProjectionRecord {
            key: key(),
            binding: ProjectionBinding::None,
            delivered: None,
            pending: None,
            last_error: None
        })
        .is_err());
    drop(lease);
    assert!(second.acquire(&malicious).is_ok());
    assert!(std::fs::read_dir(directory.path())
        .unwrap()
        .all(|entry| entry.unwrap().file_name().to_string_lossy().len() == 69));
}

struct FailCompletionStore {
    inner: FileProjectionStore,
    fail: Arc<AtomicBool>,
}
struct FailCompletionLease {
    inner: Box<dyn ProjectionLease>,
    fail: Arc<AtomicBool>,
}
impl ProjectionStore for FailCompletionStore {
    fn acquire(&self, key: &ProjectionKey) -> Result<Box<dyn ProjectionLease>, ProjectionError> {
        Ok(Box::new(FailCompletionLease {
            inner: self.inner.acquire(key)?,
            fail: self.fail.clone(),
        }))
    }
}
impl ProjectionLease for FailCompletionLease {
    fn load(&self) -> Result<Option<ProjectionRecord>, ProjectionError> {
        self.inner.load()
    }
    fn save(&self, record: &ProjectionRecord) -> Result<(), ProjectionError> {
        if record.delivered.is_some() && self.fail.swap(false, Ordering::SeqCst) {
            return Err(ProjectionError::Io(std::io::Error::other(
                "injected disk failure",
            )));
        }
        self.inner.save(record)
    }
}

#[tokio::test]
async fn completion_persistence_failure_never_claims_delivery_and_replays_durable_pending() {
    let directory = tempfile::tempdir().unwrap();
    let dispatcher = ProjectionDispatcher::new(FailCompletionStore {
        inner: FileProjectionStore::new(directory.path()),
        fail: Arc::new(AtomicBool::new(true)),
    });
    dispatcher.bind(&key(), binding()).unwrap();
    let fake = Arc::new(FakeGh::default());
    let adapter = GithubProjectProjection::with_command(fake.clone());
    assert!(matches!(
        dispatcher
            .deliver(&key(), update(1, "accepted"), &adapter)
            .await,
        Err(ProjectionError::Io(_))
    ));
    assert!(dispatcher
        .record(&key())
        .unwrap()
        .unwrap()
        .delivered
        .is_none());
    assert!(matches!(
        dispatcher.retry_pending(&key(), &adapter).await.unwrap(),
        DeliveryOutcome::Delivered { revision: 1, .. }
    ));
    assert_eq!(fake.calls.lock().unwrap().len(), 2);
    assert_eq!(fake.remote.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn missing_bindings_and_unknown_statuses_never_invent_external_resources() {
    let directory = tempfile::tempdir().unwrap();
    let dispatcher = ProjectionDispatcher::new(FileProjectionStore::new(directory.path()));
    let fake = Arc::new(FakeGh::default());
    let adapter = GithubProjectProjection::with_command(fake.clone());
    assert!(matches!(
        dispatcher
            .deliver(&key(), update(1, "proposed"), &adapter)
            .await,
        Err(ProjectionError::Unbound)
    ));
    dispatcher.bind(&key(), binding()).unwrap();
    assert!(matches!(
        dispatcher
            .deliver(&key(), update(1, "unmapped"), &adapter)
            .await
            .unwrap(),
        DeliveryOutcome::Deferred { .. }
    ));
    assert!(fake.calls.lock().unwrap().is_empty());
}
