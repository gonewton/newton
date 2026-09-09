use crate::{DependencyError, DependencyGraph, DependencyMap};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Human review record. The embedding application authenticates and authorizes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineApproval {
    /// Exact canonical map fingerprint presented to the reviewer.
    pub map_fingerprint: String,
    /// Authorized human identity; not an agent or inferred identity.
    pub reviewed_by: String,
    /// Human-readable review timestamp supplied by the caller.
    pub reviewed_at: String,
    /// Scope/completeness statement, including known cross-service limitations.
    pub completeness_statement: String,
    /// All unresolved issue IDs explicitly accepted as limitations by the reviewer.
    pub acknowledged_issues: BTreeSet<String>,
}

/// Persistable facts plus review record. Loading revalidates both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineDocument {
    /// Reviewed map rather than cached analysis results.
    pub map: DependencyMap,
    /// Approval tied to the exact map's fingerprint.
    pub approval: BaselineApproval,
}

/// Immutable reviewed map; only this type exposes automatic Impact Sequence planning.
#[derive(Debug, Clone)]
pub struct ApprovedBaseline {
    graph: DependencyGraph,
    approval: BaselineApproval,
}

impl ApprovedBaseline {
    /// Approve the exact map reviewed by an authorized human.
    ///
    /// The crate verifies content identity and explicit unresolved-input review;
    /// callers must authenticate the reviewer. Confirmation of individual edges
    /// never substitutes for this whole-map completeness review.
    pub fn approve(
        graph: DependencyGraph,
        approval: BaselineApproval,
    ) -> Result<Self, DependencyError> {
        if approval.reviewed_by.trim().is_empty()
            || approval.reviewed_at.trim().is_empty()
            || approval.completeness_statement.trim().is_empty()
        {
            return Err(DependencyError::InvalidApproval(
                "reviewer, review time, and completeness statement are required".into(),
            ));
        }
        if approval.map_fingerprint != graph.fingerprint()? {
            return Err(DependencyError::InvalidApproval(
                "the map changed after it was reviewed; review the current fingerprint".into(),
            ));
        }
        let issues = graph.map().issues.iter().map(|i| i.id.clone()).collect();
        if approval.acknowledged_issues != issues {
            return Err(DependencyError::InvalidApproval(
                "unresolved discovery issues must be explicitly acknowledged by exact ID".into(),
            ));
        }
        Ok(Self { graph, approval })
    }

    /// Restore a persisted document, rejecting altered facts or invalid approvals.
    pub fn from_document(document: BaselineDocument) -> Result<Self, DependencyError> {
        Self::approve(DependencyGraph::new(document.map)?, document.approval)
    }

    /// Export the reviewed facts and approval for caller-owned persistence.
    pub fn document(&self) -> BaselineDocument {
        BaselineDocument {
            map: self.graph.map().clone(),
            approval: self.approval.clone(),
        }
    }

    /// Read the exact graph approved for planning.
    pub fn graph(&self) -> &DependencyGraph {
        &self.graph
    }

    /// Read the authorized approval metadata and reviewed map identity.
    pub fn approval(&self) -> &BaselineApproval {
        &self.approval
    }
}
