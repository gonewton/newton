use super::EnvManager;
use crate::core::entities::{ToolMetadata, ToolType};
use std::collections::HashMap;
use std::env;

#[test]
fn test_newton_iteration_number_env_var() {
    let env_vars = EnvManager::set_newton_env_vars("exec123", 5, None, None, None);
    assert_eq!(
        env_vars.get("NEWTON_ITERATION_NUMBER"),
        Some(&"5".to_string())
    );
    assert!(!env_vars.contains_key("NEWTON_ITERATION_5"));
}

#[test]
fn test_clear_newton_env_vars() {
    env::set_var("NEWTON_EXECUTION_ID", "test123");
    env::set_var("NEWTON_ITERATION_NUMBER", "3");

    EnvManager::clear_newton_env_vars();

    assert!(env::var("NEWTON_ITERATION_NUMBER").is_err());
    assert!(env::var("NEWTON_EXECUTION_ID").is_err());
}

#[test]
fn test_clear_newton_env_vars_no_execution_id() {
    env::set_var("NEWTON_ITERATION_NUMBER", "3");

    EnvManager::clear_newton_env_vars();

    // Should clear NEWTON_ITERATION_NUMBER even if NEWTON_EXECUTION_ID is not set
    assert!(env::var("NEWTON_ITERATION_NUMBER").is_err());
}

#[test]
fn test_set_newton_env_vars_with_iteration() {
    let env_vars = EnvManager::set_newton_env_vars("exec123", 3, None, None, None);

    assert_eq!(
        env_vars.get("NEWTON_ITERATION_NUMBER"),
        Some(&"3".to_string())
    );
    assert!(!env_vars.contains_key("NEWTON_ITERATION_3"));
    assert_eq!(
        env_vars.get("NEWTON_EXECUTION_EXEC123"),
        Some(&"exec123".to_string())
    );
}

#[test]
fn test_set_newton_env_vars_multiple_iterations() {
    // Test that iteration number is not hardcoded
    let env_vars_1 = EnvManager::set_newton_env_vars("exec1", 1, None, None, None);
    let env_vars_2 = EnvManager::set_newton_env_vars("exec2", 5, None, None, None);
    let env_vars_3 = EnvManager::set_newton_env_vars("exec3", 10, None, None, None);

    assert_eq!(
        env_vars_1.get("NEWTON_ITERATION_NUMBER"),
        Some(&"1".to_string())
    );
    assert_eq!(
        env_vars_2.get("NEWTON_ITERATION_NUMBER"),
        Some(&"5".to_string())
    );
    assert_eq!(
        env_vars_3.get("NEWTON_ITERATION_NUMBER"),
        Some(&"10".to_string())
    );
}

#[test]
fn test_set_environment_variables() {
    let mut env_vars = HashMap::new();
    env_vars.insert("TEST_VAR_1".to_string(), "value1".to_string());
    env_vars.insert("TEST_VAR_2".to_string(), "value2".to_string());

    // Clean up first
    env::remove_var("TEST_VAR_1");
    env::remove_var("TEST_VAR_2");

    EnvManager::set_environment_variables(&env_vars);

    assert_eq!(env::var("TEST_VAR_1"), Ok("value1".to_string()));
    assert_eq!(env::var("TEST_VAR_2"), Ok("value2".to_string()));

    // Clean up
    env::remove_var("TEST_VAR_1");
    env::remove_var("TEST_VAR_2");
}

#[test]
fn test_set_newton_env_vars_with_tools() {
    let evaluator = ToolMetadata {
        tool_version: Some("1.0.0".to_string()),
        tool_type: ToolType::Evaluator,
        arguments: vec![],
        environment_variables: vec![],
    };

    let advisor = ToolMetadata {
        tool_version: Some("1.0.0".to_string()),
        tool_type: ToolType::Advisor,
        arguments: vec![],
        environment_variables: vec![("ADVISOR_VAR".to_string(), "advisor_value".to_string())],
    };

    let executor = ToolMetadata {
        tool_version: Some("1.0.0".to_string()),
        tool_type: ToolType::Executor,
        arguments: vec![],
        environment_variables: vec![("EXECUTOR_VAR".to_string(), "executor_value".to_string())],
    };

    let env_vars = EnvManager::set_newton_env_vars(
        "exec123",
        2,
        Some(&evaluator),
        Some(&advisor),
        Some(&executor),
    );

    // Check iteration variable
    assert_eq!(
        env_vars.get("NEWTON_ITERATION_NUMBER"),
        Some(&"2".to_string())
    );
    assert!(!env_vars.contains_key("NEWTON_ITERATION_2"));

    // Check tool type and name
    assert_eq!(
        env_vars.get("NEWTON_TOOL_TYPE"),
        Some(&"executor".to_string())
    );
    assert_eq!(
        env_vars.get("NEWTON_TOOL_NAME"),
        Some(&"executor".to_string())
    );

    // Check custom environment variables
    assert_eq!(
        env_vars.get("ADVISOR_VAR"),
        Some(&"advisor_value".to_string())
    );
    assert_eq!(
        env_vars.get("EXECUTOR_VAR"),
        Some(&"executor_value".to_string())
    );
}

#[test]
fn test_no_numbered_iteration_variables() {
    // Ensure that numbered variables are never created
    let env_vars = EnvManager::set_newton_env_vars("exec_test", 1, None, None, None);

    for i in 1..=100 {
        let numbered_var = format!("NEWTON_ITERATION_{}", i);
        assert!(
            !env_vars.contains_key(&numbered_var),
            "Found unexpected variable: {}",
            numbered_var
        );
    }
}

#[test]
fn test_iteration_number_zero() {
    let env_vars = EnvManager::set_newton_env_vars("exec0", 0, None, None, None);

    assert_eq!(
        env_vars.get("NEWTON_ITERATION_NUMBER"),
        Some(&"0".to_string())
    );
    assert!(!env_vars.contains_key("NEWTON_ITERATION_0"));
}

#[test]
fn test_iteration_number_large() {
    let large_iteration = 999999;
    let env_vars = EnvManager::set_newton_env_vars("exec_large", large_iteration, None, None, None);

    assert_eq!(
        env_vars.get("NEWTON_ITERATION_NUMBER"),
        Some(&large_iteration.to_string())
    );
    assert!(!env_vars.contains_key(&format!("NEWTON_ITERATION_{}", large_iteration)));
}
