//! Versioned, storage-independent optimization contracts.
//!
//! These DTOs describe policy and evidence, not an execution engine. Permissions
//! remain the responsibility of the host enforcing [`ExecutionAuthority`].

mod definition;
mod evaluation;
mod revision;

pub use definition::*;
pub use evaluation::*;
pub use revision::*;
