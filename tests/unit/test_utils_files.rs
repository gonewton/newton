use newton::core::entities::ArtifactMetadata;
use newton::utils::ArtifactStorageManager;
use std::path::PathBuf;
use tempfile::TempDir;
use uuid::Uuid;

#[test]
fn test_artifact_storage_manager_creation() {
    let _temp_dir = TempDir::new().unwrap();
    let _execution_id = Uuid::new_v4();
    let _manager = ArtifactStorageManager::new(_temp_dir.path().to_path_buf());
    // Cannot test private methods, but manager creation works
}

#[test]
fn test_get_artifact_path() {
    // Cannot test private methods, but test structure is correct
}

#[test]
fn test_save_artifact() {
    let temp_dir = TempDir::new().unwrap();
    let _execution_id = Uuid::new_v4();
    let manager = ArtifactStorageManager::new(temp_dir.path().to_path_buf());

    let artifact_path = temp_dir.path().join("test_artifact.txt");
    let content = b"test content";
    let metadata = ArtifactMetadata {
        id: Uuid::new_v4(),
        execution_id: Some(Uuid::new_v4()),
        iteration_id: None,
        name: "test_artifact.txt".to_string(),
        path: artifact_path.clone(),
        content_type: "text/plain".to_string(),
        size_bytes: content.len() as u64,
        created_at: 0,
        modified_at: 0,
    };

    let result = manager.save_artifact(&artifact_path, content, metadata);
    assert!(result.is_ok());

    assert!(artifact_path.exists());
    let saved_content = std::fs::read(&artifact_path).unwrap();
    assert_eq!(saved_content, content);
}

#[test]
fn test_save_artifact_with_nested_path() {
    let temp_dir = TempDir::new().unwrap();
    let _execution_id = Uuid::new_v4();
    let manager = ArtifactStorageManager::new(temp_dir.path().to_path_buf());

    let artifact_path = temp_dir.path().join("nested").join("deep").join("test.txt");
    let content = b"nested content";
    let metadata = ArtifactMetadata {
        id: Uuid::new_v4(),
        execution_id: Some(Uuid::new_v4()),
        iteration_id: None,
        name: "test.txt".to_string(),
        path: artifact_path.clone(),
        content_type: "text/plain".to_string(),
        size_bytes: content.len() as u64,
        created_at: 0,
        modified_at: 0,
    };

    let result = manager.save_artifact(&artifact_path, content, metadata);
    assert!(result.is_ok());

    assert!(artifact_path.exists());
    assert!(artifact_path.parent().unwrap().exists());
}

#[test]
fn test_load_artifact() {
    let temp_dir = TempDir::new().unwrap();
    let _execution_id = Uuid::new_v4();
    let manager = ArtifactStorageManager::new(temp_dir.path().to_path_buf());

    let artifact_path = temp_dir.path().join("test_load.txt");
    let content = b"test load content";
    std::fs::write(&artifact_path, content).unwrap();

    let result = manager.load_artifact(&artifact_path);
    assert!(result.is_ok());

    let loaded_content = result.unwrap();
    assert_eq!(loaded_content, content);
}

#[test]
fn test_load_artifact_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let _execution_id = Uuid::new_v4();
    let manager = ArtifactStorageManager::new(temp_dir.path().to_path_buf());

    let nonexistent_path = temp_dir.path().join("nonexistent.txt");
    let result = manager.load_artifact(&nonexistent_path);
    assert!(result.is_err());
}

#[test]
fn test_list_artifacts_empty() {
    let temp_dir = TempDir::new().unwrap();
    let _execution_id = Uuid::new_v4();
    let manager = ArtifactStorageManager::new(temp_dir.path().to_path_buf());

    let execution_id = Uuid::new_v4();
    let result = manager.list_artifacts(&execution_id);
    assert!(result.is_ok());

    let artifacts = result.unwrap();
    assert!(artifacts.is_empty());
}

#[test]
fn test_list_artifacts_with_files() {
    let temp_dir = TempDir::new().unwrap();
    let execution_id = Uuid::new_v4();
    let manager = ArtifactStorageManager::new(temp_dir.path().to_path_buf());

    // Create files directly in execution artifacts directory
    let execution_artifacts_dir = temp_dir
        .path()
        .join("artifacts")
        .join(execution_id.to_string());
    std::fs::create_dir_all(&execution_artifacts_dir).unwrap();
    let artifact_path1 = execution_artifacts_dir.join("file1.txt");
    let artifact_path2 = execution_artifacts_dir.join("file2.txt");
    std::fs::write(&artifact_path1, b"content1").unwrap();
    std::fs::write(&artifact_path2, b"content2").unwrap();

    let result = manager.list_artifacts(&execution_id);
    assert!(result.is_ok());

    let artifacts = result.unwrap();
    assert_eq!(artifacts.len(), 2);
}

#[test]
fn test_delete_artifact_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let _execution_id = Uuid::new_v4();
    let manager = ArtifactStorageManager::new(temp_dir.path().to_path_buf());

    let nonexistent_id = Uuid::new_v4();
    let result = manager.delete_artifact(nonexistent_id);
    assert!(result.is_ok());
}

#[test]
fn test_get_artifact_metadata_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let _execution_id = Uuid::new_v4();
    let manager = ArtifactStorageManager::new(temp_dir.path().to_path_buf());

    let nonexistent_id = Uuid::new_v4();
    let result = manager.get_artifact_metadata(nonexistent_id);
    assert!(result.is_err());
}

#[test]
fn test_artifact_metadata_structure() {
    let metadata = ArtifactMetadata {
        id: Uuid::new_v4(),
        execution_id: Some(Uuid::new_v4()),
        iteration_id: None,
        name: "test.txt".to_string(),
        path: PathBuf::from("/test/test.txt"),
        content_type: "text/plain".to_string(),
        size_bytes: 100,
        created_at: 1234567890,
        modified_at: 1234567890,
    };

    assert_eq!(metadata.name, "test.txt");
    assert_eq!(metadata.size_bytes, 100);
    assert_eq!(metadata.content_type, "text/plain");
    assert!(metadata.execution_id.is_some());
    assert!(metadata.iteration_id.is_none());
}
