use newton::core::workflow_graph::artifacts::ArtifactStore;
use newton::core::workflow_graph::schema::{ArtifactCleanupPolicy, ArtifactStorageSettings};
use newton::core::workflow_graph::state::compute_sha256_hex;
use newton::core::workflow_graph::state::OutputRef;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;
use uuid::Uuid;

fn default_settings() -> ArtifactStorageSettings {
    ArtifactStorageSettings {
        base_path: PathBuf::from(".newton/artifacts"),
        max_inline_bytes: 1 << 10,
        max_artifact_bytes: 1 << 20,
        max_total_bytes: 1 << 22,
        retention_hours: 168,
        cleanup_policy: ArtifactCleanupPolicy::Lru,
    }
}

#[test]
fn inline_outputs_stay_inline() {
    let workspace = tempdir().expect("workspace");
    let mut settings = default_settings();
    settings.max_inline_bytes = 1024;
    let mut store = ArtifactStore::new(workspace.path().to_path_buf(), &settings);
    let execution_id = Uuid::new_v4();
    let output = json!({"value": "small"});
    match store
        .route_output(&execution_id, "task_one", 1, output.clone())
        .expect("success")
    {
        OutputRef::Inline(value) => assert_eq!(value, output),
        other => panic!("expected inline, got {:?}", other),
    }
}

#[test]
fn large_outputs_route_to_artifacts() {
    let workspace = tempdir().expect("workspace");
    let mut settings = default_settings();
    settings.max_inline_bytes = 1;
    let mut store = ArtifactStore::new(workspace.path().to_path_buf(), &settings);
    let execution_id = Uuid::new_v4();
    let payload = json!({"value": "{}", "repeat": 200});
    if let OutputRef::Artifact {
        path,
        size_bytes,
        sha256,
    } = store
        .route_output(&execution_id, "task_big", 1, payload.clone())
        .expect("artifact")
    {
        assert!(size_bytes > settings.max_inline_bytes as u64);
        let artifact_path = workspace.path().join(&path);
        let bytes = fs::read(&artifact_path).expect("artifact file");
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
        assert_eq!(parsed, payload);
        assert_eq!(sha256, compute_sha256_hex(&bytes));
    } else {
        panic!("expected artifact output");
    }
}

#[test]
fn output_exceeding_artifact_limit_errors() {
    let workspace = tempdir().expect("workspace");
    let mut settings = default_settings();
    settings.max_inline_bytes = 1;
    settings.max_artifact_bytes = 4;
    let mut store = ArtifactStore::new(workspace.path().to_path_buf(), &settings);
    let execution_id = Uuid::new_v4();
    let payload = json!({"value": "{}", "repeat": 200});
    let err = store
        .route_output(&execution_id, "task_big", 1, payload)
        .expect_err("too large")
        .code;
    assert_eq!(err, "WFG-ART-002");
}

#[test]
fn invalid_task_id_rejected() {
    let workspace = tempdir().expect("workspace");
    let mut settings = default_settings();
    settings.max_inline_bytes = 1;
    let mut store = ArtifactStore::new(workspace.path().to_path_buf(), &settings);
    let execution_id = Uuid::new_v4();
    let payload = json!({"value": "ok"});
    let err = store
        .route_output(&execution_id, "../escape", 1, payload)
        .expect_err("invalid task id")
        .code;
    assert_eq!(err, "WFG-ART-001");
}
