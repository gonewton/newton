//! Dependency discovery and target-scoped release planning over approved facts.
//!
//! This crate performs no I/O, executes no agents, and assigns no versions. Callers
//! supply manifest text and catalog identities, persist baseline documents, and
//! authenticate the human who approves them. See `CONTRACT.md` for the discovery
//! support matrix, trust boundary, and per-hop compatibility semantics.

mod baseline;
mod cargo;
mod error;
mod graph;
mod impact;
mod model;

pub use baseline::{ApprovedBaseline, BaselineApproval, BaselineDocument};
pub use cargo::{discover_cargo, CargoPackage, GitReference, PackageSource};
pub use error::DependencyError;
pub use graph::DependencyGraph;
pub use impact::{ImpactModule, ImpactRequest, ImpactSequence, ReleaseGroup, ReleaseStage};
pub use model::{
    ArtifactKind, ArtifactRef, CompatibilitySignal, Dependency, DependencyKind, DependencyMap,
    Discovery, DiscoveryIssue, DiscoveryReport, EffortClass, Membership, PlannedChange, Version,
    VersionConstraint,
};
