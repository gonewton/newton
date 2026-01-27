extern crate newton;

use tempfile::TempDir;
use newton::core::{OptimizationOrchestrator, WorkspaceManager};
use newton::cli::RunArgs;
use std::path::PathBuf;
use std::process::Command;

fn create_test_workspace() -> Result<TempDir, Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let workspace_path = temp_dir.path();
    
    // Create basic workspace structure
    std::fs::create_dir_all(workspace_path.join("problem"))?;
    std::fs::create_dir_all(workspace_path.join("tools"))?;
    
    // Create problem files
    std::fs::write(
        workspace_path.join("problem/GOAL.md"),
        "# Test Goal\n\nThis is a test optimization goal."
    )?;
    
    std::fs::write(
        workspace_path.join("problem/CONSTRAINTS.md"),
        "# Test Constraints\n\nThese are test constraints."
    )?;
    
    // Create solution file
    std::fs::write(
        workspace_path.join("solution.json"),
        r#"{"value": 0, "iterations": 0}"#
    )?;
    
    // Create simple test tools
    let evaluator_script = r#"#!/bin/bash
echo "0.5" > "$NEWTON_SCORE_FILE"
echo "Evaluation completed"
"#;
    std::fs::write(workspace_path.join("tools/evaluator.sh"), evaluator_script)?;
    
    let advisor_script = r#"#!/bin/bash
echo "# Recommendations\n\nImprove the solution" > "$NEWTON_ADVISOR_DIR/recommendations.md"
echo "Advice generated"
"#;
    std::fs::write(workspace_path.join("tools/advisor.sh"), advisor_script)?;
    
    let executor_script = r#"#!/bin/bash
echo '{"value": 1, "iterations": 1}' > "$NEWTON_WORKSPACE_PATH/solution.json"
echo "Execution completed"
"#;
    std::fs::write(workspace_path.join("tools/executor.sh"), executor_script)?;
    
    // Make tools executable
    Command::new("chmod")
        .arg("+x")
        .arg(workspace_path.join("tools/evaluator.sh"))
        .arg(workspace_path.join("tools/advisor.sh"))
        .arg(workspace_path.join("tools/executor.sh"))
        .output()?;
    
    Ok(temp_dir)
}

#[tokio::test]
async fn test_full_optimization_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let temp_workspace = create_test_workspace()?;
    let workspace_path = temp_workspace.path();
    
    // Create run args
    let run_args = RunArgs {
        workspace_path: workspace_path.to_path_buf(),
        evaluator_cmd: workspace_path.join("tools/evaluator.sh"),
        advisor_cmd: workspace_path.join("tools/advisor.sh"),
        executor_cmd: workspace_path.join("tools/executor.sh"),
        max_iterations: Some(2),
        max_time_seconds: Some(60),
        tool_timeout_seconds: Some(30),
        strict_toolchain_mode: true,
        evaluator_timeout: Some(30),
        advisor_timeout: Some(30),
        executor_timeout: Some(30),
        global_timeout: Some(60),
        resource_monitoring: false,
    };
    
    // This should fail initially because orchestrator is not implemented
    let result = OptimizationOrchestrator::run_optimization(run_args).await;
    
    // For now, we expect this to fail with "not yet implemented"
    assert!(result.is_err());
    
    Ok(())
}

#[test]
fn test_workspace_validation() -> Result<(), Box<dyn std::error::Error>> {
    let temp_workspace = create_test_workspace()?;
    let workspace_path = temp_workspace.path();
    
    // Test workspace validation
    let workspace_manager = WorkspaceManager::new();
    let result = workspace_manager.validate_workspace(workspace_path);
    
    // This should succeed since we created a valid workspace
    assert!(result.is_ok());
    
    Ok(())
}

#[test]
fn test_invalid_workspace_validation() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let invalid_path = temp_dir.path().join("nonexistent");
    
    // Test workspace validation with invalid path
    let workspace_manager = WorkspaceManager::new();
    let result = workspace_manager.validate_workspace(&invalid_path);
    
    // This should fail
    assert!(result.is_err());
    
    Ok(())
}