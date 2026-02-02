use newton::core::entities::{ExecutionConfiguration, ExecutionStatus, OptimizationExecution};
use newton::core::history_recorder::ExecutionHistoryRecorder;
use tempfile::TempDir;
use uuid::Uuid;

#[test]
fn test_history_recorder_creation() {
    let temp_dir = TempDir::new().unwrap();
    let _recorder = ExecutionHistoryRecorder::new(temp_dir.path().to_path_buf());
}

#[test]
fn test_record_execution() {
    let temp_dir = TempDir::new().unwrap();
    let recorder = ExecutionHistoryRecorder::new(temp_dir.path().to_path_buf());

    let execution = create_test_execution();
    let result = recorder.record_execution(&execution);

    assert!(result.is_ok());
}

#[test]
fn test_load_execution() {
    let temp_dir = TempDir::new().unwrap();
    let recorder = ExecutionHistoryRecorder::new(temp_dir.path().to_path_buf());

    let original_execution = create_test_execution();
    recorder.record_execution(&original_execution).unwrap();

    let loaded_execution = recorder.load_execution(original_execution.execution_id);

    assert!(loaded_execution.is_ok());

    let loaded = loaded_execution.unwrap();
    assert_eq!(loaded.execution_id, original_execution.execution_id);
    assert_eq!(loaded.status, original_execution.status);
    assert_eq!(
        loaded.total_iterations_completed,
        original_execution.total_iterations_completed
    );
}

#[test]
fn test_load_nonexistent_execution() {
    let temp_dir = TempDir::new().unwrap();
    let recorder = ExecutionHistoryRecorder::new(temp_dir.path().to_path_buf());

    let nonexistent_id = Uuid::new_v4();
    let result = recorder.load_execution(nonexistent_id);

    assert!(result.is_err());
}

#[test]
fn test_record_multiple_executions() {
    let temp_dir = TempDir::new().unwrap();
    let recorder = ExecutionHistoryRecorder::new(temp_dir.path().to_path_buf());

    let executions = vec![
        create_test_execution_with_id(Uuid::new_v4()),
        create_test_execution_with_id(Uuid::new_v4()),
        create_test_execution_with_id(Uuid::new_v4()),
    ];

    for execution in &executions {
        let result = recorder.record_execution(execution);
        assert!(result.is_ok());
    }
}

#[test]
fn test_record_execution_with_nested_directories() {
    let temp_dir = TempDir::new().unwrap();
    let storage_path = temp_dir.path().join("deep").join("nested").join("storage");
    std::fs::create_dir_all(&storage_path).unwrap();

    let recorder = ExecutionHistoryRecorder::new(storage_path);
    let execution = create_test_execution();

    let result = recorder.record_execution(&execution);
    assert!(result.is_ok());
}

#[test]
fn test_load_execution_with_complex_data() {
    let temp_dir = TempDir::new().unwrap();
    let recorder = ExecutionHistoryRecorder::new(temp_dir.path().to_path_buf());

    let mut execution = create_test_execution();
    execution.iterations = vec![];
    execution.artifacts = vec![];

    recorder.record_execution(&execution).unwrap();

    let loaded_execution = recorder.load_execution(execution.execution_id).unwrap();
    assert_eq!(loaded_execution.execution_id, execution.execution_id);
    assert_eq!(
        loaded_execution.iterations.len(),
        execution.iterations.len()
    );
}

fn create_test_execution() -> OptimizationExecution {
    create_test_execution_with_id(Uuid::new_v4())
}

fn create_test_execution_with_id(execution_id: Uuid) -> OptimizationExecution {
    let config = ExecutionConfiguration {
        evaluator_cmd: Some("evaluator".to_string()),
        advisor_cmd: Some("advisor".to_string()),
        executor_cmd: Some("executor".to_string()),
        max_time_seconds: Some(300),
        max_iterations: Some(10),
        evaluator_timeout_ms: Some(5000),
        advisor_timeout_ms: Some(5000),
        executor_timeout_ms: Some(5000),
        global_timeout_ms: Some(300000),
        strict_toolchain_mode: true,
        resource_monitoring: false,
        verbose: false,
    };

    OptimizationExecution {
        id: Uuid::new_v4(),
        workspace_path: std::path::PathBuf::from("/test/workspace"),
        execution_id,
        status: ExecutionStatus::Completed,
        started_at: chrono::Utc::now(),
        completed_at: Some(chrono::Utc::now()),
        resource_limits: Default::default(),
        max_iterations: Some(10),
        current_iteration: Some(5),
        final_solution_path: Some(std::path::PathBuf::from("/test/solution.json")),
        current_iteration_path: Some(std::path::PathBuf::from("/test/current.json")),
        total_iterations_completed: 5,
        total_iterations_failed: 0,
        iterations: vec![],
        artifacts: vec![],
        configuration: config,
    }
}
