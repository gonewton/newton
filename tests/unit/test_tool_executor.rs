use newton::core::entities::{ExecutionConfiguration, ToolType};
use newton::core::tool_executor::ToolExecutor;
use std::collections::HashMap;
use tempfile::TempDir;

#[tokio::test]
async fn test_tool_executor_creation() {
    let _executor = ToolExecutor::new();
    // Should be able to create without error
}

#[tokio::test]
async fn test_tool_executor_default() {
    let _executor = ToolExecutor::new();
    // Should be able to create using Default trait
}

#[tokio::test]
async fn test_execute_simple_command() {
    let temp_dir = TempDir::new().unwrap();
    let executor = ToolExecutor::new();

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

    // Use a command that should exist on most systems
    let result = executor
        .execute(
            "echo 'hello world'",
            &config,
            &temp_dir.path().to_path_buf(),
        )
        .await;

    assert!(result.is_ok());
    let tool_result = result.unwrap();
    assert_eq!(tool_result.tool_name, "echo 'hello world'");
    assert!(tool_result.success);
    assert!(tool_result.stdout.contains("hello world"));
}

#[tokio::test]
async fn test_execute_command_with_args() {
    let temp_dir = TempDir::new().unwrap();
    let executor = ToolExecutor::new();

    let config = ExecutionConfiguration {
        evaluator_cmd: Some("test evaluator".to_string()),
        advisor_cmd: Some("test advisor".to_string()),
        executor_cmd: Some("test executor".to_string()),
        max_iterations: None,
        max_time_seconds: None,
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
            "echo 'test with args'",
            &config,
            &temp_dir.path().to_path_buf(),
        )
        .await;

    assert!(result.is_ok());
    let tool_result = result.unwrap();
    assert_eq!(tool_result.tool_name, "echo 'test with args'");
    assert!(tool_result.success);

    // Check that environment variables were set
    let env_vars: HashMap<String, String> = tool_result
        .metadata
        .environment_variables
        .iter()
        .cloned()
        .collect();
    assert_eq!(
        env_vars.get("NEWTON_EVALUATOR_CMD"),
        Some(&"test evaluator".to_string())
    );
    assert_eq!(
        env_vars.get("NEWTON_ADVISOR_CMD"),
        Some(&"test advisor".to_string())
    );
    assert_eq!(
        env_vars.get("NEWTON_EXECUTOR_CMD"),
        Some(&"test executor".to_string())
    );
    assert!(env_vars.contains_key("NEWTON_EVALUATOR_TIMEOUT_MS"));
    assert!(env_vars.contains_key("NEWTON_ADVISOR_TIMEOUT_MS"));
    assert!(env_vars.contains_key("NEWTON_EXECUTOR_TIMEOUT_MS"));
    assert!(env_vars.contains_key("NEWTON_WORKSPACE_PATH"));
    assert!(env_vars.contains_key("NEWTON_ITERATION_ID"));
    assert!(env_vars.contains_key("NEWTON_EXECUTION_ID"));
}

#[tokio::test]
async fn test_execute_failing_command() {
    let temp_dir = TempDir::new().unwrap();
    let executor = ToolExecutor::new();

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

    // Use a command that should fail
    let result = executor
        .execute("/usr/bin/false", &config, &temp_dir.path().to_path_buf())
        .await;

    assert!(result.is_ok());
    let tool_result = result.unwrap();
    assert_eq!(tool_result.tool_name, "/usr/bin/false");
    assert!(!tool_result.success);
    assert_eq!(tool_result.exit_code, 1);
    assert!(tool_result.error.is_some());
}

#[tokio::test]
async fn test_execute_nonexistent_command() {
    let temp_dir = TempDir::new().unwrap();
    let executor = ToolExecutor::new();

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

    // Use a command that doesn't exist
    let result = executor
        .execute(
            "nonexistent_command_12345",
            &config,
            &temp_dir.path().to_path_buf(),
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_tool_result_structure() {
    let temp_dir = TempDir::new().unwrap();
    let executor = ToolExecutor::new();

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

    let result = executor
        .execute("echo 'test'", &config, &temp_dir.path().to_path_buf())
        .await;

    assert!(result.is_ok());
    let tool_result = result.unwrap();

    assert_eq!(tool_result.tool_name, "echo 'test'");
    assert_eq!(tool_result.exit_code, 0);
    assert!(tool_result.success);
    assert!(tool_result.error.is_none());
    assert!(matches!(
        tool_result.metadata.tool_type,
        ToolType::Evaluator | ToolType::Advisor | ToolType::Executor
    ));
    // Note: Very fast commands may complete in 0ms
    assert!(!tool_result.stdout.is_empty());

    // Check arguments parsing
    assert_eq!(tool_result.metadata.arguments.len(), 1); // "test"

    // Check environment variables contain basic required ones
    let env_vars: HashMap<String, String> = tool_result
        .metadata
        .environment_variables
        .iter()
        .cloned()
        .collect();
    assert_eq!(
        env_vars.get("NEWTON_WORKSPACE_PATH"),
        Some(&temp_dir.path().to_string_lossy().to_string())
    );
}

#[tokio::test]
async fn test_environment_variables_comprehensive() {
    let temp_dir = TempDir::new().unwrap();
    let executor = ToolExecutor::new();

    let config = ExecutionConfiguration {
        evaluator_cmd: Some("custom_evaluator".to_string()),
        advisor_cmd: Some("custom_advisor".to_string()),
        executor_cmd: Some("custom_executor".to_string()),
        max_iterations: Some(50),
        max_time_seconds: Some(600),
        evaluator_timeout_ms: Some(10000),
        advisor_timeout_ms: Some(15000),
        executor_timeout_ms: Some(20000),
        global_timeout_ms: Some(600000),
        strict_toolchain_mode: true,
        resource_monitoring: true,
        verbose: true,
    };

    let result = executor
        .execute("echo 'env test'", &config, &temp_dir.path().to_path_buf())
        .await;

    assert!(result.is_ok());
    let tool_result = result.unwrap();

    // Check all environment variables are set correctly
    let env_vars: HashMap<String, String> = tool_result
        .metadata
        .environment_variables
        .iter()
        .cloned()
        .collect();
    assert_eq!(
        env_vars.get("NEWTON_EVALUATOR_CMD"),
        Some(&"custom_evaluator".to_string())
    );
    assert_eq!(
        env_vars.get("NEWTON_ADVISOR_CMD"),
        Some(&"custom_advisor".to_string())
    );
    assert_eq!(
        env_vars.get("NEWTON_EXECUTOR_CMD"),
        Some(&"custom_executor".to_string())
    );
    // Check for timeout variables
    assert!(env_vars.contains_key("NEWTON_EVALUATOR_TIMEOUT_MS"));
    assert!(env_vars.contains_key("NEWTON_ADVISOR_TIMEOUT_MS"));
    assert!(env_vars.contains_key("NEWTON_EXECUTOR_TIMEOUT_MS"));
    // Check for workspace path
    assert!(env_vars.contains_key("NEWTON_WORKSPACE_PATH"));
    // Get the workspace path value
    let workspace_path_value = env_vars
        .get("NEWTON_WORKSPACE_PATH")
        .expect("NEWTON_WORKSPACE_PATH should exist")
        .clone();
    assert_eq!(
        workspace_path_value,
        temp_dir.path().to_string_lossy().to_string()
    );
}
