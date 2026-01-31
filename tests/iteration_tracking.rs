use newton::core::entities::{ToolMetadata, ToolType};
use newton::utils::EnvManager;
use std::env;

#[test]
fn test_iteration_tracking_across_workflow() {
    // This test verifies that NEWTON_ITERATION_NUMBER is properly set during execution
    // Since we can't easily test the full workflow without a complete setup,
    // we'll test the core functionality through the EnvManager

    let env_vars = EnvManager::set_newton_env_vars("test_exec_123", 1, None, None, None);

    // Verify the iteration number is set correctly
    assert_eq!(
        env_vars.get("NEWTON_ITERATION_NUMBER"),
        Some(&"1".to_string())
    );

    // Verify no numbered variables are created
    assert!(!env_vars.contains_key("NEWTON_ITERATION_1"));

    // Test with different iteration numbers
    for i in 1..=5 {
        let env_vars =
            EnvManager::set_newton_env_vars(&format!("test_exec_{}", i), i, None, None, None);

        assert_eq!(
            env_vars.get("NEWTON_ITERATION_NUMBER"),
            Some(&i.to_string())
        );
        assert!(!env_vars.contains_key(&format!("NEWTON_ITERATION_{}", i)));
    }
}

#[test]
fn test_iteration_persistence_across_calls() {
    // Test that iteration number is consistently set across multiple calls
    let exec_id = "persistent_test";

    // First iteration
    let env_vars_1 = EnvManager::set_newton_env_vars(exec_id, 1, None, None, None);
    assert_eq!(
        env_vars_1.get("NEWTON_ITERATION_NUMBER"),
        Some(&"1".to_string())
    );

    // Second iteration
    let env_vars_2 = EnvManager::set_newton_env_vars(exec_id, 2, None, None, None);
    assert_eq!(
        env_vars_2.get("NEWTON_ITERATION_NUMBER"),
        Some(&"2".to_string())
    );

    // Third iteration
    let env_vars_3 = EnvManager::set_newton_env_vars(exec_id, 3, None, None, None);
    assert_eq!(
        env_vars_3.get("NEWTON_ITERATION_NUMBER"),
        Some(&"3".to_string())
    );
}

#[test]
fn test_iteration_with_tools() {
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
        environment_variables: vec![("TEST_ADVISOR_VAR".to_string(), "test_value".to_string())],
    };

    let executor = ToolMetadata {
        tool_version: Some("1.0.0".to_string()),
        tool_type: ToolType::Executor,
        arguments: vec![],
        environment_variables: vec![(
            "TEST_EXECUTOR_VAR".to_string(),
            "executor_value".to_string(),
        )],
    };

    // Test iteration 7 with all tools
    let env_vars = EnvManager::set_newton_env_vars(
        "tools_test",
        7,
        Some(&evaluator),
        Some(&advisor),
        Some(&executor),
    );

    // Verify iteration number is correct
    assert_eq!(
        env_vars.get("NEWTON_ITERATION_NUMBER"),
        Some(&"7".to_string())
    );
    assert!(!env_vars.contains_key("NEWTON_ITERATION_7"));

    // Verify tool-specific variables are also set
    assert_eq!(
        env_vars.get("NEWTON_TOOL_TYPE"),
        Some(&"executor".to_string())
    );
    assert_eq!(
        env_vars.get("NEWTON_TOOL_NAME"),
        Some(&"executor".to_string())
    );
    assert_eq!(
        env_vars.get("TEST_ADVISOR_VAR"),
        Some(&"test_value".to_string())
    );
    assert_eq!(
        env_vars.get("TEST_EXECUTOR_VAR"),
        Some(&"executor_value".to_string())
    );
}

#[test]
fn test_environment_variable_cleanup() {
    // Test that environment variables are properly cleaned up
    env::set_var("NEWTON_EXECUTION_ID", "cleanup_test");
    env::set_var("NEWTON_ITERATION_NUMBER", "999");

    // Verify variables are set
    assert_eq!(env::var("NEWTON_ITERATION_NUMBER"), Ok("999".to_string()));
    assert_eq!(
        env::var("NEWTON_EXECUTION_ID"),
        Ok("cleanup_test".to_string())
    );

    // Clear variables
    EnvManager::clear_newton_env_vars();

    // Verify variables are cleared
    assert!(env::var("NEWTON_ITERATION_NUMBER").is_err());
    assert!(env::var("NEWTON_EXECUTION_ID").is_err());
}

#[test]
fn test_iteration_number_edge_cases() {
    // Test iteration number 0
    let env_vars_0 = EnvManager::set_newton_env_vars("edge_test", 0, None, None, None);
    assert_eq!(
        env_vars_0.get("NEWTON_ITERATION_NUMBER"),
        Some(&"0".to_string())
    );
    assert!(!env_vars_0.contains_key("NEWTON_ITERATION_0"));

    // Test large iteration number
    let large_iter = 1000000;
    let env_vars_large =
        EnvManager::set_newton_env_vars("large_test", large_iter, None, None, None);
    assert_eq!(
        env_vars_large.get("NEWTON_ITERATION_NUMBER"),
        Some(&large_iter.to_string())
    );
    assert!(!env_vars_large.contains_key(&format!("NEWTON_ITERATION_{}", large_iter)));
}
