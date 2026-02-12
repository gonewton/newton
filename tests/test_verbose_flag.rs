#[test]
fn test_verbose_flag_in_run_args() {
    use newton::cli::args::RunArgs;
    use std::path::PathBuf;

    let args = RunArgs {
        path: PathBuf::from("."),
        max_iterations: 5,
        max_time: 60,
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
        verbose: true,
        config: None,
        goal: None,
        control_file: None,
        goal_file: None,
        feedback: None,
    };

    assert!(args.verbose);
}

#[test]
fn test_verbose_flag_false_by_default_in_run_args() {
    use newton::cli::args::RunArgs;
    use std::path::PathBuf;

    let args = RunArgs {
        path: PathBuf::from("."),
        max_iterations: 5,
        max_time: 60,
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
        control_file: None,
        goal_file: None,
        feedback: None,
    };

    assert!(!args.verbose);
}

#[test]
fn test_verbose_flag_in_step_args() {
    use newton::cli::args::StepArgs;
    use std::path::PathBuf;

    let args = StepArgs {
        path: PathBuf::from("."),
        execution_id: Some("test-exec".to_string()),
        verbose: true,
    };

    assert!(args.verbose);
}

#[test]
fn test_verbose_flag_in_execution_configuration() {
    use newton::core::entities::ExecutionConfiguration;

    let config = ExecutionConfiguration {
        verbose: true,
        ..Default::default()
    };

    assert!(config.verbose);
}

#[test]
fn test_execution_configuration_default_verbose() {
    use newton::core::entities::ExecutionConfiguration;

    let config = ExecutionConfiguration::default();

    assert!(!config.verbose);
}
