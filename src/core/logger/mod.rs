pub struct Tracer;

impl Tracer {
    pub fn new() -> Self {
        Tracer
    }

    pub fn trace(&self, message: &str) {
        println!("[TRACING] {}", message);
    }
}

impl Default for Tracer {
    fn default() -> Self {
        Self::new()
    }
}
