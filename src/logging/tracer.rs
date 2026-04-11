/// Simple tracer for basic message tracing.
pub struct Tracer;

impl Tracer {
    /// Create a new tracer instance.
    pub fn new() -> Self {
        Tracer
    }

    /// Emit a trace message to standard output.
    pub fn trace(&self, message: &str) {
        println!("[TRACING] {}", message);
    }
}

impl Default for Tracer {
    fn default() -> Self {
        Self::new()
    }
}
