//! Optional, write-only status projections of Newton-owned entity state.
//!
//! Projection failures do not decide optimization work. The durable delivery
//! journal is separate from domain state; adapters cannot read external state.

mod dispatcher;
mod error;
mod github;
mod model;
mod port;
mod service;
mod store;

pub use dispatcher::ProjectionDispatcher;
pub use error::ProjectionError;
pub use github::{GhCommand, GithubProjectProjection};
pub use model::{
    DeliveryOutcome, ProjectionBinding, ProjectionKey, ProjectionReceipt, ProjectionRecord,
    ProjectionUpdate,
};
pub use port::{NoTracker, ProjectionPort};
pub use service::{
    RunProjectionConfiguration, RunProjectionDestination, RunProjectionReport,
    RunProjectionService, RunProjectionSnapshot, TargetProjectionReport,
};
pub use store::{FileProjectionStore, ProjectionLease, ProjectionStore};
