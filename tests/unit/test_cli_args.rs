use newton::cli::args::{RunArgs, StepArgs, StatusArgs, ReportArgs, ErrorArgs, ReportFormat};
use std::path::PathBuf;

#[test]
fn test_run_args_default_path() {
    let args = RunArgs {
        path: PathBuf::from("."),
        max_iterations: 10,
        max_time: 300,
        evaluator_cmd: None,
        advisor_cmd: None,
        executor_cmd: None,
        evaluator_status_file: PathBuf::from("artifacts/evaluator_status.md"),
        advisor_recommendations_file: PathBuf::from("artifacts/advisor_recommendations.md"),
        executor_log_file: PathBuf::from("artifacts/executor_log.md"),
        tool_timeout_seconds: 30,
        evaluator_timeout: None,
        advisor_timeout: None,
        executor_timeout: None,
        verbose: false,
        config: None,
        goal: None,
        goal_file: None,
        control_file: None,
        feedback: None,
    };
    
    assert_eq!(args.max_iterations, 10);
    assert_eq!(args.max_time, 300);
}

#[test]
fn test_run_args_with_commands() {
    let args = RunArgs {
        path: PathBuf::from("."),
        max_iterations: 5,
        max_time: 60,
        evaluator_cmd: Some("./evaluator.sh".to_string()),
        advisor_cmd: Some("./advisor.sh".to_string()),
        executor_cmd: Some("./executor.sh".to_string()),
        evaluator_status_file: PathBuf::from("artifacts/evaluator_status.md"),
        advisor_recommendations_file: PathBuf::from("artifacts/advisor_recommendations.md"),
        executor_log_file: PathBuf::from("artifacts/executor_log.md"),
        tool_timeout_seconds: 30,
        evaluator_timeout: Some(10),
        advisor_timeout: Some(20),
        executor_timeout: Some(30),
        verbose: false,
        config: None,
        goal: None,
        goal_file: None,
        control_file: None,
        feedback: None,
    };
    
    assert!(args.evaluator_cmd.is_some());
    assert!(args.advisor_cmd.is_some());
    assert!(args.executor_cmd.is_some());
    assert_eq!(args.max_iterations, 5);
}

#[test]
fn test_step_args() {
    let args = StepArgs {
        path: PathBuf::from("."),
        execution_id: Some("test-exec".to_string()),
    };
    
    assert_eq!(args.path, PathBuf::from("."));
    assert_eq!(args.execution_id, Some("test-exec".to_string()));
}

#[test]
fn test_status_args() {
    let args = StatusArgs {
        execution_id: "test-exec-123".to_string(),
        path: PathBuf::from("."),
    };
    
    assert_eq!(args.execution_id, "test-exec-123");
    assert_eq!(args.path, PathBuf::from("."));
}

#[test]
fn test_report_args() {
    let args = ReportArgs {
        execution_id: "test-exec-123".to_string(),
        path: PathBuf::from("."),
        format: ReportFormat::Json,
    };
    
    assert_eq!(args.execution_id, "test-exec-123");
    assert_eq!(args.format, ReportFormat::Json);
}

#[test]
fn test_report_format_variants() {
    let text_format = ReportFormat::Text;
    let json_format = ReportFormat::Json;
    
    match text_format {
        ReportFormat::Text => {},
        ReportFormat::Json => panic!("Unexpected format"),
    }
    
    match json_format {
        ReportFormat::Text => panic!("Unexpected format"),
        ReportFormat::Json => {},
    }
}

#[test]
fn test_error_args() {
    let args = ErrorArgs {
        execution_id: "test-exec-123".to_string(),
        verbose: true,
    };
    
    assert_eq!(args.execution_id, "test-exec-123");
    assert!(args.verbose);
}
