//! Allowed `CommandSpec.category` values for Newton (spec §4.1).
//!
//! cli-framework stores `category` as `&'static str`; pinning the allowed
//! set here lets the registry-uniqueness test refuse drift.

pub const WORKFLOW: &str = "workflow";
pub const OPS: &str = "ops";
pub const MAINTENANCE: &str = "maintenance";
pub const WORKSPACE: &str = "workspace";
pub const OPERATIONAL: &str = "operational";

pub const ALL: &[&str] = &[WORKFLOW, OPS, MAINTENANCE, WORKSPACE, OPERATIONAL];

pub fn is_allowed(category: &str) -> bool {
    ALL.contains(&category)
}
