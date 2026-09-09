use super::*;
use newton_core::optimization::{
    bind_definition, parse_definition, DefinitionBinding, EnforcementCapabilities,
    OptimizationObservationError, RunObservationReason,
};
use std::{collections::BTreeMap, path::Path, time::Duration};

fn binding(workspace: &Path, project: &str) -> BoundOptimizationDefinition {
    let definition = parse_definition(include_str!(
        "../../../../../tests/fixtures/optimization/definition.yaml"
    ))
    .unwrap();
    let authority = ExecutionAuthority {
        allowed_actions: [ExecutionAction::Merge].into(),
    };
    bind_definition(
        &definition,
        DefinitionBinding {
            run_id: uuid::Uuid::new_v4().to_string(),
            context: OptimizationContext {
                id: project.into(),
                root: workspace.display().to_string(),
            },
            project_parameters: BTreeMap::new(),
            run_overrides: BTreeMap::new(),
            project_authority: authority.clone(),
            environment_authority: authority,
            enforcement: EnforcementCapabilities::default(),
        },
    )
    .unwrap()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/optimization")
}

async fn update(
    observer: &mut OptimizeRunObservation,
) -> newton_core::optimization::RunObservationUpdate {
    tokio::time::timeout(Duration::from_secs(5), observer.next_update())
        .await
        .expect("real native driver must publish a scoped update")
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn actual_driver_supplies_initial_snapshot_updates_and_refresh_without_server_or_tracker() {
    let directory = tempfile::tempdir().unwrap();
    let binding = binding(directory.path(), "observed");
    let run_id = binding.run_id.clone();
    let state = directory.path().join(".newton/state");
    let (run, mut observer) =
        ObservedOptimizationRun::start(binding, fixtures(), state.clone(), 4096)
            .await
            .unwrap();
    assert_eq!(observer.snapshot().detail.run.id, run_id);
    assert_eq!(observer.snapshot().detail.run.status, "running");
    assert_eq!(observer.snapshot().detail.run.cycle, 0);
    assert!(observer.snapshot().cycles.is_empty());

    let result = run.run(false, 0).await.unwrap();
    assert_eq!(result.completion.status, CheckStatus::Satisfied);
    let changed = update(&mut observer).await;
    assert_eq!(changed.reason, RunObservationReason::Changed);
    assert_eq!(changed.snapshot.detail.run.id, run_id);
    assert_eq!(changed.snapshot.detail.run.status, "converged");
    assert!(!changed.snapshot.cycles.is_empty());
    assert!(changed
        .snapshot
        .cycles
        .iter()
        .all(|cycle| cycle.run_id == run_id));
    let stored_outcome: OptimizationOutcome =
        serde_json::from_value(changed.snapshot.detail.outcome_reason.unwrap()).unwrap();
    assert_eq!(stored_outcome.run_id, result.run_id);
    assert_eq!(stored_outcome.stop_reason, result.stop_reason);
    assert_eq!(
        observer.refresh().await.unwrap().reason,
        RunObservationReason::Refreshed
    );
    assert!(!state.join("projections").exists());
}

#[tokio::test]
async fn actual_driver_channel_overflow_recovers_terminal_durable_trajectory() {
    let directory = tempfile::tempdir().unwrap();
    let binding = binding(directory.path(), "lagged");
    let run_id = binding.run_id.clone();
    let (run, mut observer) = ObservedOptimizationRun::start(
        binding,
        fixtures(),
        directory.path().join(".newton/state"),
        1,
    )
    .await
    .unwrap();
    let outcome = run.run(false, 0).await.unwrap();
    let recovered = update(&mut observer).await;
    assert_eq!(recovered.reason, RunObservationReason::LagRecovered);
    assert_eq!(recovered.snapshot.detail.run.id, run_id);
    assert_eq!(recovered.snapshot.detail.run.status, "converged");
    assert_eq!(
        recovered.snapshot.detail.run.cycle as u64,
        outcome.usage.cycles
    );
    assert_eq!(recovered.snapshot.cycles.len() as u64, outcome.usage.cycles);
    assert_eq!(
        update(&mut observer).await.reason,
        RunObservationReason::Changed
    );
    assert!(observer.next_update().await.unwrap().is_none());
    // Closing a publisher is not itself completion; the durable outcome is.
    assert!(observer.snapshot().detail.outcome_reason.is_some());
}

#[tokio::test]
async fn shared_real_publisher_never_delivers_another_runs_incremental_stream() {
    let directory = tempfile::tempdir().unwrap();
    let first_context = directory.path().join("first");
    let second_context = directory.path().join("second");
    std::fs::create_dir_all(&first_context).unwrap();
    std::fs::create_dir_all(&second_context).unwrap();
    let state = directory.path().join("state");
    let (publisher, _) = broadcast::channel(4096);
    let (first, mut first_observer) = ObservedOptimizationRun::start_with_publisher(
        binding(&first_context, "first"),
        fixtures(),
        state.clone(),
        publisher.clone(),
    )
    .await
    .unwrap();
    let first_id = first_observer.snapshot().detail.run.id.clone();
    let (second, second_observer) = ObservedOptimizationRun::start_with_publisher(
        binding(&second_context, "second"),
        fixtures(),
        state,
        publisher.clone(),
    )
    .await
    .unwrap();
    let second_id = second_observer.snapshot().detail.run.id.clone();
    assert_ne!(first_id, second_id);
    second.run(false, 0).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), first_observer.next_update())
            .await
            .is_err()
    );
    assert_eq!(first_observer.snapshot().detail.run.status, "running");
    first.run(false, 0).await.unwrap();
    let changed = update(&mut first_observer).await;
    assert_eq!(changed.reason, RunObservationReason::Changed);
    assert_eq!(changed.snapshot.detail.run.id, first_id);
    assert!(changed
        .snapshot
        .cycles
        .iter()
        .all(|cycle| cycle.run_id == first_id));
}

#[tokio::test]
async fn observer_errors_and_disconnects_cannot_control_the_driver() {
    let directory = tempfile::tempdir().unwrap();
    let (run, observer) = ObservedOptimizationRun::start(
        binding(directory.path(), "errors"),
        fixtures(),
        directory.path().join(".newton/state"),
        16,
    )
    .await
    .unwrap();
    let source = run.driver.observation_source();
    assert!(matches!(
        source.subscribe("").await,
        Err(OptimizationObservationError::MissingRun)
    ));
    assert!(matches!(
        source.subscribe("missing-run").await,
        Err(OptimizationObservationError::Store(_))
    ));
    drop(source);
    drop(observer);
    assert_eq!(
        run.run(false, 0).await.unwrap().completion.status,
        CheckStatus::Satisfied
    );
}

#[tokio::test]
async fn invalid_buffer_capacity_is_rejected_before_run_state_is_created() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join(".newton/state");
    for capacity in [0, 4097] {
        let result = ObservedOptimizationRun::start(
            binding(directory.path(), "bounded"),
            fixtures(),
            state.clone(),
            capacity,
        )
        .await;
        assert!(result.is_err());
        assert!(!state.exists());
    }
}

#[tokio::test]
async fn actual_resource_stop_is_observed_as_resource_limit_not_completion() {
    let directory = tempfile::tempdir().unwrap();
    let mut binding = binding(directory.path(), "resource-limited");
    binding.definition.requirements.resource_limits.max_cycles = 1;
    binding.definition.requirements.completion = vec![CompletionCriterion::ObjectiveTarget {
        objective: "size".into(),
        target: 0.0,
    }];
    binding.requirements.requirements = binding.definition.requirements.clone();
    let (run, mut observer) = ObservedOptimizationRun::start(
        binding,
        fixtures(),
        directory.path().join(".newton/state"),
        4096,
    )
    .await
    .unwrap();
    let outcome = run.run(false, 0).await.unwrap();
    assert_eq!(outcome.stop_reason, OptimizationStopReason::ResourceLimit);
    assert_ne!(outcome.completion.status, CheckStatus::Satisfied);
    assert!(outcome.accepted_result.is_some());
    let changed = update(&mut observer).await;
    assert_eq!(changed.snapshot.detail.run.status, "resource_limit");
    assert_eq!(
        changed.snapshot.detail.outcome_reason.unwrap()["stop_reason"],
        "resource_limit"
    );
}
