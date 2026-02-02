use newton::core::orchestrator::OptimizationOrchestrator;
use newton::core::entities::{ExecutionConfiguration, OptimizationExecution, ExecutionStatus};
use newton::core::error::DefaultErrorReporter;
use newton::utils::serialization::{JsonSerializer, FileUtils, Serializer, FileSerializer};
use tempfile::TempDir;

#[tokio::test]
async fn test_orchestrator_creation() {
    let serializer = JsonSerializer;
    let file_serializer = FileUtils;
    let reporter = Box::new(DefaultErrorReporter);
    
    let orchestrator = OptimizationOrchestrator::new(serializer, file_serializer, reporter);
    
    // Test that the orchestrator has the expected components
    assert!(orchestrator.serializer().serialize(&"test").is_ok());
}

#[tokio::test]
async fn test_orchestrator_minimal_execution() {
    let temp_dir = TempDir::new().unwrap();
    let serializer = JsonSerializer;
    let file_serializer = FileUtils;
    let reporter = Box::new(DefaultErrorReporter);
    
    let orchestrator = OptimizationOrchestrator::new(serializer, file_serializer, reporter);
    
    let config = ExecutionConfiguration {
        evaluator_cmd: None,
        advisor_cmd: None,
        executor_cmd: None,
        max_iterations: Some(1),
        max_time_seconds: Some(5),
        evaluator_timeout_ms: None,
        advisor_timeout_ms: None,
        executor_timeout_ms: None,
        global_timeout_ms: Some(5000),
        strict_toolchain_mode: false,
        resource_monitoring: false,
        verbose: false,
    };
    
    let result = orchestrator.run_optimization(temp_dir.path(), config).await;
    assert!(result.is_ok());
    
    let execution = result.unwrap();
    assert_eq!(execution.status, ExecutionStatus::Completed);
    assert_eq!(execution.total_iterations_completed, 1);
    assert!(execution.completed_at.is_some());
}

#[tokio::test]
async fn test_orchestrator_zero_iterations() {
    let temp_dir = TempDir::new().unwrap();
    let serializer = JsonSerializer;
    let file_serializer = FileUtils;
    let reporter = Box::new(DefaultErrorReporter);
    
    let orchestrator = OptimizationOrchestrator::new(serializer, file_serializer, reporter);
    
    let config = ExecutionConfiguration {
        evaluator_cmd: None,
        advisor_cmd: None,
        executor_cmd: None,
        max_iterations: Some(0),
        max_time_seconds: Some(10),
        evaluator_timeout_ms: None,
        advisor_timeout_ms: None,
        executor_timeout_ms: None,
        global_timeout_ms: Some(10000),
        strict_toolchain_mode: false,
        resource_monitoring: false,
        verbose: false,
    };
    
    let result = orchestrator.run_optimization(temp_dir.path(), config).await;
    assert!(result.is_ok());
    
    let execution = result.unwrap();
    assert_eq!(execution.status, ExecutionStatus::Completed);
    assert_eq!(execution.total_iterations_completed, 0);
}

#[test]
fn test_execution_configuration_all_fields() {
    let config = ExecutionConfiguration {
        evaluator_cmd: Some("test_evaluator".to_string()),
        advisor_cmd: Some("test_advisor".to_string()),
        executor_cmd: Some("test_executor".to_string()),
        max_iterations: Some(100),
        max_time_seconds: Some(3600),
        evaluator_timeout_ms: Some(30000),
        advisor_timeout_ms: Some(30000),
        executor_timeout_ms: Some(30000),
        global_timeout_ms: Some(3600000),
        strict_toolchain_mode: true,
        resource_monitoring: true,
        verbose: true,
    };
    
    assert_eq!(config.evaluator_cmd, Some("test_evaluator".to_string()));
    assert_eq!(config.max_iterations, Some(100));
    assert_eq!(config.max_time_seconds, Some(3600));
    assert_eq!(config.evaluator_timeout_ms, Some(30000));
    assert!(config.strict_toolchain_mode);
    assert!(config.resource_monitoring);
    assert!(config.verbose);
}

#[test]
fn test_file_operations() {
    let temp_dir = TempDir::new().unwrap();
    let file_utils = FileUtils;
    let serializer = JsonSerializer;
    
    let test_data = "test content";
    let file_path = temp_dir.path().join("test_file.txt");
    
    // Test save operation
    let save_result = file_utils.save_to_file(&file_path, &test_data, &serializer);
    assert!(save_result.is_ok());
    assert!(file_path.exists());
    
    // Test load operation
    let load_result: Result<String, _> = file_utils.load_from_file(&file_path, &serializer);
    assert!(load_result.is_ok());
    assert_eq!(load_result.unwrap(), test_data);
}

#[test]
fn test_json_serialization() {
    let serializer = JsonSerializer;
    
    let test_data = serde_json::json!({
        "name": "test",
        "value": 42,
        "items": ["a", "b", "c"]
    });
    
    let serialized = serializer.serialize(&test_data).unwrap();
    assert!(!serialized.is_empty());
    
    let deserialized: serde_json::Value = serializer.deserialize(&serialized).unwrap();
    assert_eq!(test_data, deserialized);
}
