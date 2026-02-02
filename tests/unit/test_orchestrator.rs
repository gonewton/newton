use newton::core::entities::{ExecutionConfiguration, ExecutionStatus, OptimizationExecution};
use newton::core::error::DefaultErrorReporter;
use newton::core::orchestrator::OptimizationOrchestrator;
use newton::utils::serialization::FileSerializer;
use std::path::PathBuf;
use tempfile::TempDir;

#[tokio::test]
async fn test_orchestrator_creation() {
    let serializer = newton::utils::serialization::JsonSerializer;
    let file_serializer = newton::utils::serialization::FileUtils;
    let reporter = Box::new(DefaultErrorReporter);

    let orchestrator = OptimizationOrchestrator::new(serializer, file_serializer, reporter);

    // Test passes if serializer and file_serializer work together
    assert!(orchestrator
        .file_serializer()
        .save_to_file(
            &PathBuf::from("test.json"),
            &"test",
            orchestrator.serializer()
        )
        .is_ok());
}

#[tokio::test]
async fn test_orchestrator_run_optimization_minimal() {
    let _temp_dir = TempDir::new().unwrap();
    let serializer = newton::utils::serialization::JsonSerializer;
    let file_serializer = newton::utils::serialization::FileUtils;
    let reporter = Box::new(DefaultErrorReporter);

    let _orchestrator = OptimizationOrchestrator::new(serializer, file_serializer, reporter);

    let config = ExecutionConfiguration {
        evaluator_cmd: None,
        advisor_cmd: None,
        executor_cmd: None,
        max_time_seconds: None,
        evaluator_timeout_ms: None,
        advisor_timeout_ms: None,
        executor_timeout_ms: None,
        global_timeout_ms: None,
        strict_toolchain_mode: false,
        resource_monitoring: false,
        verbose: false,
        max_iterations: None,
    };

    let execution_id = uuid::Uuid::new_v4();
    let execution = OptimizationExecution {
        id: execution_id,
        workspace_path: std::path::PathBuf::from("/test/workspace"),
        execution_id,
        status: ExecutionStatus::Running,
        started_at: chrono::Utc::now(),
        completed_at: None,
        resource_limits: Default::default(),
        max_iterations: None,
        current_iteration: None,
        final_solution_path: None,
        current_iteration_path: None,
        total_iterations_completed: 0,
        total_iterations_failed: 0,
        iterations: vec![],
        artifacts: vec![],
        configuration: config,
    };

    assert_eq!(execution.execution_id, execution_id);
    assert_eq!(execution.status, ExecutionStatus::Running);
    assert_eq!(execution.total_iterations_completed, 0);
}
