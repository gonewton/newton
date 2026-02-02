use newton::core::entities::{ExecutionConfiguration, ToolType};

#[tokio::test]
async fn test_tool_types_coverage() {
    let tool_types = vec![ToolType::Evaluator, ToolType::Advisor, ToolType::Executor];
    for tool_type in &tool_types {
        assert!(matches!(
            tool_type,
            ToolType::Evaluator | ToolType::Advisor | ToolType::Executor
        ));
    }
}

#[tokio::test]
async fn test_execution_configuration_strict_mode() {
    let config = ExecutionConfiguration {
        evaluator_cmd: Some("test".to_string()),
        advisor_cmd: Some("test".to_string()),
        executor_cmd: Some("test".to_string()),
        max_time_seconds: Some(300),
        evaluator_timeout_ms: Some(5000),
        advisor_timeout_ms: Some(5000),
        executor_timeout_ms: Some(5000),
        global_timeout_ms: Some(300000),
        strict_toolchain_mode: true,
        resource_monitoring: false,
        verbose: false,
        max_iterations: None,
    };

    assert!(config.strict_toolchain_mode);
    assert!(config.evaluator_cmd.is_some());
    assert!(config.advisor_cmd.is_some());
    assert!(config.executor_cmd.is_some());
}

#[tokio::test]
async fn test_execution_configuration_non_strict_mode() {
    let config = ExecutionConfiguration {
        evaluator_cmd: None,
        advisor_cmd: None,
        executor_cmd: None,
        max_time_seconds: Some(150),
        evaluator_timeout_ms: None,
        advisor_timeout_ms: None,
        executor_timeout_ms: None,
        global_timeout_ms: Some(150000),
        strict_toolchain_mode: false,
        resource_monitoring: false,
        verbose: true,
        max_iterations: None,
    };

    assert!(!config.strict_toolchain_mode);
    assert!(config.evaluator_cmd.is_none());
    assert!(config.advisor_cmd.is_none());
    assert!(config.executor_cmd.is_none());
    assert!(config.verbose);
}

#[tokio::test]
async fn test_timeout_configurations() {
    let config = ExecutionConfiguration {
        evaluator_cmd: None,
        advisor_cmd: None,
        executor_cmd: None,
        max_time_seconds: None,
        evaluator_timeout_ms: Some(10000),
        advisor_timeout_ms: Some(20000),
        executor_timeout_ms: Some(30000),
        global_timeout_ms: Some(60000),
        strict_toolchain_mode: false,
        resource_monitoring: false,
        verbose: false,
        max_iterations: None,
    };

    assert_eq!(config.evaluator_timeout_ms, Some(10000));
    assert_eq!(config.advisor_timeout_ms, Some(20000));
    assert_eq!(config.executor_timeout_ms, Some(30000));
    assert_eq!(config.global_timeout_ms, Some(60000));
}

#[tokio::test]
async fn test_configuration_edge_cases() {
    let config = ExecutionConfiguration {
        evaluator_cmd: Some("a".repeat(1000)),
        advisor_cmd: Some("b".repeat(1000)),
        executor_cmd: Some("c".repeat(1000)),
        max_time_seconds: None,
        evaluator_timeout_ms: None,
        advisor_timeout_ms: None,
        executor_timeout_ms: None,
        global_timeout_ms: None,
        strict_toolchain_mode: true,
        resource_monitoring: true,
        verbose: true,
        max_iterations: None,
    };

    assert_eq!(config.max_time_seconds, None);
    assert_eq!(config.evaluator_cmd.as_ref().unwrap().len(), 1000);
}

#[tokio::test]
async fn test_configuration_zero_values() {
    let config = ExecutionConfiguration {
        evaluator_cmd: Some("".to_string()),
        advisor_cmd: Some("".to_string()),
        executor_cmd: Some("".to_string()),
        max_time_seconds: Some(0),
        evaluator_timeout_ms: Some(0),
        advisor_timeout_ms: Some(0),
        executor_timeout_ms: Some(0),
        global_timeout_ms: Some(0),
        strict_toolchain_mode: false,
        resource_monitoring: false,
        verbose: false,
        max_iterations: None,
    };

    assert_eq!(config.max_time_seconds, Some(0));
    assert_eq!(config.evaluator_timeout_ms, Some(0));
    assert_eq!(config.advisor_timeout_ms, Some(0));
    assert_eq!(config.executor_timeout_ms, Some(0));
    assert_eq!(config.global_timeout_ms, Some(0));
    assert!(config.evaluator_cmd.as_ref().unwrap().is_empty());
}
