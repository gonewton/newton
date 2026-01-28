use newton::core::PerformanceProfiler;

#[test]
fn test_performance_profiler_creation() {
    let profiler = PerformanceProfiler::new();
    assert_eq!(profiler.record_count(), 0);
}

#[test]
fn test_performance_profiler_record() {
    let mut profiler = PerformanceProfiler::new();
    profiler.record("test_operation", std::time::Duration::from_millis(100));
    assert_eq!(profiler.record_count(), 1);
}

#[test]
fn test_performance_profiler_multiple_records() {
    let mut profiler = PerformanceProfiler::new();
    profiler.record("op1", std::time::Duration::from_millis(50));
    profiler.record("op2", std::time::Duration::from_millis(75));
    profiler.record("op1", std::time::Duration::from_millis(100));
    assert_eq!(profiler.record_count(), 3);
}

#[test]
fn test_performance_profiler_summary() {
    let mut profiler = PerformanceProfiler::new();
    profiler.record("fast_op", std::time::Duration::from_millis(10));
    profiler.record("fast_op", std::time::Duration::from_millis(20));
    profiler.record("slow_op", std::time::Duration::from_millis(100));
    
    let summary = profiler.get_summary();
    assert!(summary.contains("fast_op"));
    assert!(summary.contains("slow_op"));
}
