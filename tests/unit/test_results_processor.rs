use newton::core::ResultsProcessor;
use newton::core::OutputFormat;

#[test]
fn test_results_processor_creation() {
    let processor = ResultsProcessor::new();
    assert_eq!(processor.execution_count(), 0);
}

#[test]
fn test_results_processor_add_execution() {
    let mut processor = ResultsProcessor::new();
    let execution = newton::core::entities::OptimizationExecution {
        id: uuid::Uuid::new_v4(),
        workspace_path: std::path::PathBuf::from("/test"),
        execution_id: uuid::Uuid::new_v4(),
        status: newton::core::entities::ExecutionStatus::Completed,
        started_at: chrono::Utc::now(),
        completed_at: Some(chrono::Utc::now()),
        resource_limits: Default::default(),
        max_iterations: Some(10),
        current_iteration: Some(10),
        final_solution_path: None,
        current_iteration_path: None,
        total_iterations_completed: 10,
        total_iterations_failed: 0,
        iterations: vec![],
        artifacts: vec![],
        configuration: Default::default(),
    };
    
    processor.add_execution(execution);
    assert_eq!(processor.execution_count(), 1);
}

#[test]
fn test_results_processor_text_format() {
    let processor = ResultsProcessor::new();
    let execution = newton::core::entities::OptimizationExecution {
        id: uuid::Uuid::new_v4(),
        workspace_path: std::path::PathBuf::from("/test"),
        execution_id: uuid::Uuid::new_v4(),
        status: newton::core::entities::ExecutionStatus::Completed,
        started_at: chrono::Utc::now(),
        completed_at: Some(chrono::Utc::now()),
        resource_limits: Default::default(),
        max_iterations: Some(10),
        current_iteration: Some(10),
        final_solution_path: None,
        current_iteration_path: None,
        total_iterations_completed: 10,
        total_iterations_failed: 0,
        iterations: vec![],
        artifacts: vec![],
        configuration: Default::default(),
    };
    
    processor.add_execution(execution);
    let output = processor.generate_report(OutputFormat::Text);
    assert!(output.contains("Completed"));
}
