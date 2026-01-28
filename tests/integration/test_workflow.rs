extern crate newton;

use tempfile::TempDir;
use newton::core::OptimizationOrchestrator;
use newton::utils::serialization::{JsonSerializer, FileUtils};
use newton::core::error::DefaultErrorReporter;
use std::path::PathBuf;
use std::process::Command;

fn create_test_workspace() -> Result<TempDir, Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let workspace_path = temp_dir.path();
    
    // Create tools directory and executable scripts only
    std::fs::create_dir_all(workspace_path.join("tools"))?;
    
    // Create simple test tools
    let evaluator_script = r#"#!/bin/bash
echo "0.5" > "$NEWTON_SCORE_FILE"
echo "Evaluation completed"
"#;
    std::fs::write(workspace_path.join("tools/evaluator.sh"), evaluator_script)?;
    
    let advisor_script = "#!/bin/bash\necho \"# Recommendations\\n\\nImprove solution\" > \"$NEWTON_ADVISOR_DIR/recommendations.md\"\necho \"Advice generated\"\n";
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
    
    // Create execution configuration
    let config = newton::core::entities::ExecutionConfiguration {
        evaluator_cmd: Some(workspace_path.join("tools/evaluator.sh").to_string_lossy().to_string()),
        advisor_cmd: Some(workspace_path.join("tools/advisor.sh").to_string_lossy().to_string()),
        executor_cmd: Some(workspace_path.join("tools/executor.sh").to_string_lossy().to_string()),
        max_iterations: Some(2),
        max_time_seconds: Some(60),
        evaluator_timeout_ms: Some(30000),
        advisor_timeout_ms: Some(30000),
        executor_timeout_ms: Some(30000),
        global_timeout_ms: Some(60000),
        strict_toolchain_mode: true,
        resource_monitoring: false,
        verbose: false,
    };
    
    // Create orchestrator and run optimization
    let serializer = JsonSerializer;
    let file_serializer = FileUtils;
    let reporter = Box::new(DefaultErrorReporter);
    let orchestrator = OptimizationOrchestrator::new(serializer, file_serializer, reporter);
    
    let result = orchestrator.run_optimization(workspace_path, config).await;
    
    // Should succeed now that workspace validation is removed
    assert!(result.is_ok());
    
    Ok(())
}