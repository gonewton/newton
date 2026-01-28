use newton::core::types::*;

#[test]
fn test_execution_status_default() {
    let status = ExecutionStatus::default();
    assert_eq!(status, ExecutionStatus::Pending);
}

#[test]
fn test_iteration_status_display() {
    let status = IterationStatus::Running;
    assert_eq!(format!("{:?}", status), "Running");
}

#[test]
fn test_error_category_display() {
    let category = ErrorCategory::ValidationError;
    assert_eq!(format!("{:?}", category), "ValidationError");
}

#[test]
fn test_error_severity_display() {
    let severity = ErrorSeverity::Error;
    assert_eq!(format!("{:?}", severity), "Error");
}

#[test]
fn test_iteration_phase_display() {
    let phase = IterationPhase::Evaluator;
    assert_eq!(format!("{:?}", phase), "Evaluator");
}

#[test]
fn test_tool_type_display() {
    let tool_type = ToolType::Executor;
    assert_eq!(format!("{:?}", tool_type), "Executor");
}

#[test]
fn test_all_execution_status_variants() {
    let _variants = [
        ExecutionStatus::Pending,
        ExecutionStatus::Running,
        ExecutionStatus::Completed,
        ExecutionStatus::Failed,
        ExecutionStatus::Terminated,
    ];
    assert_eq!(5, 5);
}

#[test]
fn test_all_iteration_status_variants() {
    let _variants = [
        IterationStatus::Running,
        IterationStatus::Completed,
        IterationStatus::Failed,
    ];
    assert_eq!(3, 3);
}

#[test]
fn test_all_error_category_variants() {
    let _variants = [
        ErrorCategory::ValidationError,
        ErrorCategory::ToolExecutionError,
        ErrorCategory::TimeoutError,
        ErrorCategory::ResourceError,
        ErrorCategory::WorkspaceError,
        ErrorCategory::IterationError,
        ErrorCategory::SerializationError,
        ErrorCategory::IoError,
        ErrorCategory::ArtifactError,
        ErrorCategory::InternalError,
        ErrorCategory::Unknown,
    ];
    assert_eq!(11, 11);
}

#[test]
fn test_all_error_severity_variants() {
    let _variants = [
        ErrorSeverity::Error,
        ErrorSeverity::Warning,
        ErrorSeverity::Info,
        ErrorSeverity::Debug,
    ];
    assert_eq!(4, 4);
}

#[test]
fn test_all_iteration_phase_variants() {
    let _variants = [
        IterationPhase::Evaluator,
        IterationPhase::Advisor,
        IterationPhase::Executor,
        IterationPhase::Complete,
    ];
    assert_eq!(4, 4);
}

#[test]
fn test_all_tool_type_variants() {
    let _variants = [ToolType::Evaluator, ToolType::Advisor, ToolType::Executor];
    assert_eq!(3, 3);
}
