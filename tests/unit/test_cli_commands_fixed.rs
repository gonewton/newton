use newton::cli::RunArgs;
use newton::core::entities::ExecutionConfiguration;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_run_args_structure() {
    let args = RunArgs {
        path: Some(PathBuf::from("/tmp")),
        max_iterations: 10,
        max_time: 300,
        evaluator_cmd: Some("echo test".to_string()),
        advisor_cmd: None,
        executor_cmd: None,
        evaluator_status_file: PathBuf::from("evaluator_status.md"),
        advisor_recommendations_file: PathBuf::from("advisor_recommendations.md"),
        executor_log_file: PathBuf::from("executor_log.md"),
        tool_timeout_seconds: 30,
        evaluator_timeout: Some(5),
        advisor_timeout: None,
        executor_timeout: None,
        verbose: false,
        config: None,
        goal: None,
        goal_file: None,
        control_file: None,
        feedback: None,
    };
    
    assert_eq!(args.max_iterations, 10);
    assert_eq!(args.max_time, 300);
    assert_eq!(args.evaluator_cmd, Some("echo test".to_string()));
    assert!(args.advisor_cmd.is_none());
    assert!(!args.verbose);
}

#[test]
fn test_execution_configuration_from_args() {
    let config = ExecutionConfiguration {
        evaluator_cmd: Some("test evaluator".to_string()),
        advisor_cmd: Some("test advisor".to_string()),
        executor_cmd: Some("test executor".to_string()),
        max_iterations: Some(5),
        max_time_seconds: Some(150),
        evaluator_timeout_ms: Some(5000),
        advisor_timeout_ms: Some(5000),
        executor_timeout_ms: Some(5000),
        global_timeout_ms: Some(150000),
        strict_toolchain_mode: true,
        resource_monitoring: false,
        verbose: true,
    };
    
    assert_eq!(config.evaluator_cmd, Some("test evaluator".to_string()));
    assert_eq!(config.max_iterations, Some(5));
    assert_eq!(config.max_time_seconds, Some(150));
    assert!(config.strict_toolchain_mode);
    assert!(config.verbose);
}

#[test] 
fn test_execution_configuration_minimal() {
    let config = ExecutionConfiguration {
        evaluator_cmd: None,
        advisor_cmd: None,
        executor_cmd: None,
        max_iterations: None,
        max_time_seconds: None,
        evaluator_timeout_ms: None,
        advisor_timeout_ms: None,
        executor_timeout_ms: None,
        global_timeout_ms: None,
        strict_toolchain_mode: false,
        resource_monitoring: false,
        verbose: false,
    };
    
    assert!(config.evaluator_cmd.is_none());
    assert!(config.advisor_cmd.is_none());
    assert!(config.executor_cmd.is_none());
    assert!(!config.strict_toolchain_mode);
    assert!(!config.verbose);
}

#[test]
fn test_path_operations() {
    let temp_dir = TempDir::new().unwrap();
    let path1 = temp_dir.path();
    let path2 = PathBuf::from("test");
    
    // Test path operations used in commands
    assert!(path1.exists());
    assert!(!path2.exists());
    
    let combined = path1.join("nested").join("file.txt");
    assert!(combined.to_string_lossy().contains("nested"));
}
