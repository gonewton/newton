/// Re-export Tracer from the logging module for backward compatibility.
/// New code should use `crate::logging::Tracer` or `crate::logging::tracer::Tracer`.
pub use crate::logging::tracer::Tracer;
