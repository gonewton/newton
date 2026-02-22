use std::time::{Duration, Instant};

pub struct PerformanceProfiler {
    start_time: Instant,
    measurements: Vec<PerformanceMeasurement>,
    pending_measurements: Vec<(String, Instant)>,
}

pub struct PerformanceMeasurement {
    pub name: String,
    pub duration: Duration,
    pub memory_usage: Option<usize>,
}

impl PerformanceProfiler {
    pub fn new() -> Self {
        PerformanceProfiler {
            start_time: Instant::now(),
            measurements: Vec::new(),
            pending_measurements: Vec::new(),
        }
    }

    pub fn start_measurement(&mut self, name: &str) {
        self.pending_measurements
            .push((name.to_string(), Instant::now()));
    }

    pub fn end_measurement(&mut self, name: &str) {
        let duration = if let Some(pos) = self
            .pending_measurements
            .iter()
            .rposition(|(pending_name, _)| pending_name == name)
        {
            let (_, start_time) = self.pending_measurements.remove(pos);
            start_time.elapsed()
        } else {
            Duration::ZERO
        };

        self.measurements.push(PerformanceMeasurement {
            name: name.to_string(),
            duration,
            memory_usage: Some(monitor_memory_usage()),
        });
    }

    pub fn get_total_time(&self) -> Duration {
        self.start_time.elapsed()
    }

    pub fn get_measurements(&self) -> &[PerformanceMeasurement] {
        &self.measurements
    }
}

impl Default for PerformanceProfiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory profiling is not yet implemented in a cross-platform manner.
/// The stub returns 0 to indicate the value is unavailable.
pub fn monitor_memory_usage() -> usize {
    0
}

/// Memory optimization is highly platform specific; the current implementation is no-op.
pub fn optimize_memory_usage() {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread::sleep, time::Duration};

    #[test]
    fn duration_is_non_zero_after_sleep() {
        let mut profiler = PerformanceProfiler::new();
        profiler.start_measurement("op");
        sleep(Duration::from_millis(10));
        profiler.end_measurement("op");

        let measurement = &profiler.get_measurements()[0];
        assert!(measurement.duration > Duration::ZERO);
    }

    #[test]
    fn total_time_increases() {
        let profiler = PerformanceProfiler::new();
        let initial = profiler.get_total_time();
        sleep(Duration::from_millis(5));
        assert!(profiler.get_total_time() >= initial);
    }

    #[test]
    fn end_without_start_records_zero() {
        let mut profiler = PerformanceProfiler::new();
        profiler.end_measurement("missing");
        let measurement = &profiler.get_measurements()[0];
        assert_eq!(measurement.duration, Duration::ZERO);
    }
}
