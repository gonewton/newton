use newton::core::entities::{
    ExecutionConfiguration, ExecutionStatus, OptimizationExecution, ResourceLimits,
};
use std::path::PathBuf;

#[test]
fn test_optimization_execution_creation() {
    let execution_id = uuid::Uuid::new_v4();
    let workspace_path = PathBuf::from("/test/workspace");

    let execution = OptimizationExecution {
        id: uuid::Uuid::new_v4(),
        workspace_path: workspace_path.clone(),
        execution_id,
        status: ExecutionStatus::Pending,
        started_at: chrono::Utc::now(),
        completed_at: None,
        resource_limits: ResourceLimits {
            max_iterations: Some(10),
            max_time_seconds: None,
            max_memory_mb: None,
            max_disk_space_mb: None,
        },
        max_iterations: Some(10),
        current_iteration: None,
        final_solution_path: None,
        current_iteration_path: None,
        total_iterations_completed: 0,
        total_iterations_failed: 0,
        iterations: vec![],
        artifacts: vec![],
        configuration: ExecutionConfiguration::default(),
    };

    assert_eq!(execution.id, execution.id);
    assert_eq!(execution.workspace_path, workspace_path);
    assert_eq!(execution.status, ExecutionStatus::Pending);
    assert_eq!(execution.max_iterations, Some(10));
}

#[test]
fn test_execution_status_transitions() {
    let mut execution = OptimizationExecution {
        id: uuid::Uuid::new_v4(),
        workspace_path: PathBuf::from("/tmp/test"),
        execution_id: uuid::Uuid::new_v4(),
        status: ExecutionStatus::Pending,
        started_at: chrono::Utc::now(),
        completed_at: None,
        resource_limits: ResourceLimits::default(),
        max_iterations: None,
        current_iteration: None,
        final_solution_path: None,
        current_iteration_path: None,
        total_iterations_completed: 0,
        total_iterations_failed: 0,
        iterations: vec![],
        artifacts: vec![],
        configuration: ExecutionConfiguration::default(),
    };

    // Test pending -> running transition
    execution.status = ExecutionStatus::Running;
    assert_eq!(execution.status, ExecutionStatus::Running);

    // Test running -> completed transition
    execution.status = ExecutionStatus::Completed;
    assert_eq!(execution.status, ExecutionStatus::Completed);
}

#[test]
fn test_resource_limits() {
    let limits = ResourceLimits {
        max_iterations: Some(100),
        max_time_seconds: Some(3600),
        max_memory_mb: Some(1024),
        max_disk_space_mb: Some(2048),
    };

    assert_eq!(limits.max_iterations, Some(100));
    assert_eq!(limits.max_time_seconds, Some(3600));
    assert_eq!(limits.max_memory_mb, Some(1024));
    assert_eq!(limits.max_disk_space_mb, Some(2048));
}

#[test]
fn test_execution_configuration() {
    let config = ExecutionConfiguration {
        evaluator_cmd: Some("./evaluator.sh".to_string()),
        advisor_cmd: Some("./advisor.sh".to_string()),
        executor_cmd: Some("./executor.sh".to_string()),
        evaluator_timeout_ms: Some(30000),
        advisor_timeout_ms: Some(45000),
        executor_timeout_ms: Some(60000),
        global_timeout_ms: Some(300000),
        max_iterations: Some(50),
        max_time_seconds: Some(1800),
        strict_toolchain_mode: true,
        resource_monitoring: false,
        verbose: false,
    };

    assert_eq!(config.evaluator_cmd, Some("./evaluator.sh".to_string()));
    assert_eq!(config.advisor_cmd, Some("./advisor.sh".to_string()));
    assert_eq!(config.executor_cmd, Some("./executor.sh".to_string()));
    assert_eq!(config.evaluator_timeout_ms, Some(30000));
    assert_eq!(config.advisor_timeout_ms, Some(45000));
    assert_eq!(config.executor_timeout_ms, Some(60000));
    assert_eq!(config.global_timeout_ms, Some(300000));
    assert_eq!(config.max_iterations, Some(50));
    assert_eq!(config.max_time_seconds, Some(1800));
    assert!(config.strict_toolchain_mode);
    assert!(!config.resource_monitoring);
}
