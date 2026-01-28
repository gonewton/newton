use newton::tools::{ToolExecution, ToolResult};
use newton::core::types::ToolType;

#[test]
fn test_tool_result_creation() {
    let result = ToolResult {
        tool_name: "test_tool".to_string(),
        exit_code: 0,
        execution_time_ms: 100,
        stdout: "success".to_string(),
        stderr: "".to_string(),
        success: true,
        error: None,
        metadata: newton::core::entities::ToolMetadata {
            tool_version: Some("1.0".to_string()),
            tool_type: ToolType::Executor,
            arguments: vec!["--test".to_string()],
            environment_variables: vec![("KEY".to_string(), "VALUE".to_string())],
        },
    };
    
    assert_eq!(result.tool_name, "test_tool");
    assert_eq!(result.exit_code, 0);
    assert!(result.success);
}

#[test]
fn test_tool_result_failure() {
    let result = ToolResult {
        tool_name: "test_tool".to_string(),
        exit_code: 1,
        execution_time_ms: 100,
        stdout: "".to_string(),
        stderr: "error".to_string(),
        success: false,
        error: Some("Test error".to_string()),
        metadata: newton::core::entities::ToolMetadata {
            tool_version: None,
            tool_type: ToolType::Executor,
            arguments: vec![],
            environment_variables: vec![],
        },
    };
    
    assert_eq!(result.exit_code, 1);
    assert!(!result.success);
    assert!(result.error.is_some());
}

#[test]
fn test_tool_result_with_metadata() {
    let metadata = newton::core::entities::ToolMetadata {
        tool_version: Some("2.0".to_string()),
        tool_type: ToolType::Evaluator,
        arguments: vec!["--verbose".to_string(), "--output".to_string()],
        environment_variables: vec![
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("HOME".to_string(), "/home/user".to_string()),
        ],
    };
    
    assert_eq!(metadata.tool_version, Some("2.0".to_string()));
    assert_eq!(metadata.tool_type, ToolType::Evaluator);
    assert_eq!(metadata.arguments.len(), 2);
    assert_eq!(metadata.environment_variables.len(), 2);
}
