use newton::core::PerformanceProfiler;

#[test]
fn test_performance_profiler_creation() {
    let _profiler = PerformanceProfiler::new();
}

#[test]
fn test_performance_profiler_start_measurement() {
    let mut profiler = PerformanceProfiler::new();
    profiler.start_measurement("test_operation");
}

#[test]
fn test_performance_profiler_end_measurement() {
    let mut profiler = PerformanceProfiler::new();
    profiler.start_measurement("test_operation");
    profiler.end_measurement("test_operation");
}
