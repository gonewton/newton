use newton::core::entities::{ExecutionConfiguration, ExecutionStatus};
use newton::core::error::{DefaultErrorReporter, ErrorReporter};
use newton::core::history_recorder::ExecutionHistoryRecorder;
use newton::core::logger::Tracer;
use newton::core::orchestrator::OptimizationOrchestrator;
use newton::core::tool_executor::ToolExecutor;
use std::path::PathBuf;
use tempfile::TempDir;
use tokio_test;

#[tokio::test]
async fn test_full_orchestrator_workflow() {
    let temp_dir = TempDir::new().unwrap();
    let serializer = newton::utils::serialization::JsonSerializer;
    let file_serializer = newton::utils::serialization::FileUtils;
    let reporter = Box::new(DefaultErrorReporter);

    let orchestrator = OptimizationOrchestrator::new(serializer, file_serializer, reporter);

    let config = ExecutionConfiguration {
        evaluator_cmd: None,
        advisor_cmd: None,
        executor_cmd: None,
        max_time_seconds: Some(10),
        max_iterations: Some(2),
        evaluator_timeout_ms: None,
        advisor_timeout_ms: None,
        executor_timeout_ms: None,
        global_timeout_ms: Some(10000),
        strict_toolchain_mode: false,
        resource_monitoring: false,
        verbose: true,
    };

    let result = orchestrator.run_optimization(temp_dir.path(), config).await;
    assert!(result.is_ok());

    let execution = result.unwrap();
    assert_eq!(execution.status, ExecutionStatus::Completed);
    assert_eq!(execution.total_iterations_completed, 2);
    assert!(execution.completed_at.is_some());
}

#[tokio::test]
async fn test_orchestrator_with_timeout() {
    let temp_dir = TempDir::new().unwrap();
    let serializer = newton::utils::serialization::JsonSerializer;
    let file_serializer = newton::utils::serialization::FileUtils;
    let reporter = Box::new(DefaultErrorReporter);

    let orchestrator = OptimizationOrchestrator::new(serializer, file_serializer, reporter);

    let config = ExecutionConfiguration {
        evaluator_cmd: Some("echo 'test evaluator'".to_string()),
        advisor_cmd: Some("echo 'test advisor'".to_string()),
        executor_cmd: Some("echo 'test executor'".to_string()),
        max_time_seconds: Some(5),
        max_iterations: Some(1),
        evaluator_timeout_ms: Some(2000),
        advisor_timeout_ms: Some(2000),
        executor_timeout_ms: Some(2000),
        global_timeout_ms: Some(5000),
        strict_toolchain_mode: true,
        resource_monitoring: false,
        verbose: false,
    };

    let result = orchestrator.run_optimization(temp_dir.path(), config).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_history_recorder_integration() {
    let temp_dir = TempDir::new().unwrap();
    let recorder = ExecutionHistoryRecorder::new(temp_dir.path().to_path_buf());

    let execution = create_test_execution();
    let record_result = recorder.record_execution(&execution);
    assert!(record_result.is_ok());

    let load_result = recorder.load_execution(execution.execution_id);
    assert!(load_result.is_ok());

    let loaded_execution = load_result.unwrap();
    assert_eq!(loaded_execution.execution_id, execution.execution_id);
    assert_eq!(loaded_execution.status, execution.status);
    assert_eq!(
        loaded_execution.total_iterations_completed,
        execution.total_iterations_completed
    );
}

#[tokio::test]
async fn test_tool_executor_integration() {
    let temp_dir = TempDir::new().unwrap();
    let executor = ToolExecutor::new();

    let config = ExecutionConfiguration {
        evaluator_cmd: Some("test_eval".to_string()),
        advisor_cmd: Some("test_adv".to_string()),
        executor_cmd: Some("test_exec".to_string()),
        max_time_seconds: None,
        max_iterations: Some(1),
        evaluator_timeout_ms: Some(5000),
        advisor_timeout_ms: Some(5000),
        executor_timeout_ms: Some(5000),
        global_timeout_ms: None,
        strict_toolchain_mode: false,
        resource_monitoring: false,
        verbose: false,
    };

    let result = executor
        .execute(
            "echo 'integration test'",
            &config,
            &temp_dir.path().to_path_buf(),
        )
        .await;
    assert!(result.is_ok());

    let tool_result = result.unwrap();
    assert!(tool_result.success);
    assert!(tool_result.stdout.contains("integration test"));

    let env_vars: Vec<(String, String)> = tool_result.metadata.environment_variables;
    assert!(env_vars
        .iter()
        .any(|(k, v)| k == "NEWTON_EVALUATOR_CMD" && v == "test_eval"));
    assert!(env_vars
        .iter()
        .any(|(k, v)| k == "NEWTON_ADVISOR_CMD" && v == "test_adv"));
    assert!(env_vars
        .iter()
        .any(|(k, v)| k == "NEWTON_EXECUTOR_CMD" && v == "test_exec"));
}

#[tokio::test]
async fn test_artifact_storage_integration() {
    let temp_dir = TempDir::new().unwrap();
    let manager = newton::utils::ArtifactStorageManager::new(temp_dir.path().to_path_buf());

    let execution_id = uuid::Uuid::new_v4();

    let artifact1_path = temp_dir
        .path()
        .join("artifacts")
        .join(&execution_id.to_string())
        .join("artifact1.txt");
    let artifact1_content = b"artifact 1 content";
    let metadata1 = newton::core::entities::ArtifactMetadata {
        id: uuid::Uuid::new_v4(),
        execution_id: Some(execution_id),
        iteration_id: Some(uuid::Uuid::new_v4()),
        name: "artifact1.txt".to_string(),
        path: artifact1_path.clone(),
        content_type: "text/plain".to_string(),
        size_bytes: artifact1_content.len() as u64,
        created_at: 0,
        modified_at: 0,
    };

    let save_result = manager.save_artifact(&artifact1_path, artifact1_content, metadata1);
    assert!(save_result.is_ok());

    let load_result = manager.load_artifact(&artifact1_path);
    assert!(load_result.is_ok());

    let loaded_content = load_result.unwrap();
    assert_eq!(loaded_content, artifact1_content);

    let list_result = manager.list_artifacts(&execution_id);
    assert!(list_result.is_ok());

    let artifacts = list_result.unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].name, "artifact1.txt");
}

#[tokio::test]
async fn test_error_handling_integration() {
    use newton::core::error::AppError;

    let reporter = DefaultErrorReporter::new();
    let tracer = Tracer::new();

    let mut error = AppError::new(
        newton::core::types::ErrorCategory::ValidationError,
        "integration test error",
    )
    .with_context("test context")
    .with_code("INT-001");

    error.add_context("test_key", "test_value");

    reporter.report_error(&error);
    reporter.report_warning("test warning", Some("test context".to_string()));
    reporter.report_info("test info");
    reporter.report_debug("test debug");

    tracer.trace("integration trace message");

    let anyhow_error = anyhow::anyhow!("test anyhow error");
    let converted_error: AppError = anyhow_error.into();
    assert_eq!(
        converted_error.category,
        newton::core::types::ErrorCategory::InternalError
    );

    let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "test file not found");
    let io_converted: AppError = io_error.into();
    assert_eq!(
        io_converted.category,
        newton::core::types::ErrorCategory::IoError
    );
}

#[tokio::test]
async fn test_complete_workflow() {
    let temp_dir = TempDir::new().unwrap();

    let serializer = newton::utils::serialization::JsonSerializer;
    let file_serializer = newton::utils::serialization::FileUtils;
    let reporter = Box::new(DefaultErrorReporter);
    let orchestrator = OptimizationOrchestrator::new(serializer, file_serializer, reporter);
    let history_recorder = ExecutionHistoryRecorder::new(temp_dir.path().to_path_buf());
    let artifact_manager =
        newton::utils::ArtifactStorageManager::new(temp_dir.path().to_path_buf());
    let tracer = Tracer::new();

    let config = ExecutionConfiguration {
        evaluator_cmd: None,
        advisor_cmd: None,
        executor_cmd: None,
        max_time_seconds: Some(5),
        max_iterations: Some(1),
        evaluator_timeout_ms: None,
        advisor_timeout_ms: None,
        executor_timeout_ms: None,
        global_timeout_ms: Some(5000),
        strict_toolchain_mode: false,
        resource_monitoring: false,
        verbose: false,
    };

    let execution_result = orchestrator.run_optimization(temp_dir.path(), config).await;
    assert!(execution_result.is_ok());

    let execution = execution_result.unwrap();

    tracer.trace("Recording execution");
    let record_result = history_recorder.record_execution(&execution);
    assert!(record_result.is_ok());

    let artifact_path = temp_dir
        .path()
        .join("artifacts")
        .join(&execution.execution_id.to_string())
        .join("workflow_artifact.txt");
    let artifact_content = format!("Execution ID: {}", execution.execution_id);
    let metadata = newton::core::entities::ArtifactMetadata {
        id: uuid::Uuid::new_v4(),
        execution_id: Some(execution.execution_id),
        iteration_id: Some(uuid::Uuid::new_v4()),
        name: "workflow_artifact.txt".to_string(),
        path: artifact_path.clone(),
        content_type: "text/plain".to_string(),
        size_bytes: artifact_content.len() as u64,
        created_at: 0,
        modified_at: 0,
    };

    let save_result =
        artifact_manager.save_artifact(&artifact_path, artifact_content.as_bytes(), metadata);
    assert!(save_result.is_ok());

    tracer.trace("Verifying artifacts");
    let list_result = artifact_manager.list_artifacts(&execution.execution_id);
    assert!(list_result.is_ok());

    let artifacts = list_result.unwrap();
    assert_eq!(artifacts.len(), 1);

    tracer.trace("Complete workflow test finished");
}

fn create_test_execution() -> newton::core::entities::OptimizationExecution {
    use newton::core::entities::*;

    let config = ExecutionConfiguration {
        evaluator_cmd: None,
        advisor_cmd: None,
        executor_cmd: None,
        max_time_seconds: None,
        max_iterations: Some(1),
        evaluator_timeout_ms: None,
        advisor_timeout_ms: None,
        executor_timeout_ms: None,
        global_timeout_ms: None,
        strict_toolchain_mode: false,
        resource_monitoring: false,
        verbose: false,
    };

    OptimizationExecution {
        id: uuid::Uuid::new_v4(),
        workspace_path: std::path::PathBuf::from("/test/workspace"),
        execution_id: uuid::Uuid::new_v4(),
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
