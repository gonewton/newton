use newton::core::error::DefaultErrorReporter;
use newton::core::types::ExecutionStatus;
use newton::core::OutputFormat;
use newton::core::ResultsProcessor;
use newton::utils::serialization::JsonSerializer;

#[test]
fn test_results_processor_creation() {
    let serializer = JsonSerializer;
    let reporter = Box::new(DefaultErrorReporter);
    let _processor = ResultsProcessor::new(serializer, reporter);
}

#[test]
fn test_results_processor_generate_report() {
    let serializer = JsonSerializer;
    let reporter = Box::new(DefaultErrorReporter);
    let processor = ResultsProcessor::new(serializer, reporter);

    let execution = newton::core::entities::OptimizationExecution {
        id: uuid::Uuid::new_v4(),
        workspace_path: std::path::PathBuf::from("/test"),
        execution_id: uuid::Uuid::new_v4(),
        status: ExecutionStatus::Completed,
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

    let output = processor.generate_report(&execution, OutputFormat::Text);
    assert!(output.is_ok());
}
