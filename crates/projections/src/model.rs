use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Internal entity plus projection destination; never derived from external text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionKey {
    /// Newton entity kind, for example `change_request` or `plan`.
    pub entity_kind: String,
    /// Canonical Newton entity identifier.
    pub entity_id: String,
    /// Configured destination identity, independent of remote status.
    pub destination: String,
}

/// Pre-authorized external identity. Initial resource creation is not supported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectionBinding {
    /// No external service is configured or required.
    None,
    /// Existing GitHub Project item and its status field/options.
    GithubProjectItem {
        /// GitHub Project node ID.
        project_id: String,
        /// Existing Project item node ID, stored durably before any delivery.
        item_id: String,
        /// Single-select status field node ID.
        field_id: String,
        /// Newton-derived status to configured single-select option ID.
        status_options: BTreeMap<String, String>,
    },
}

/// Immutable projection payload derived solely from Newton's internal state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionUpdate {
    /// Monotonically increasing entity revision supplied by the domain owner.
    pub revision: u64,
    /// Internal status to reflect; external board edits cannot supply it.
    pub derived_status: String,
}

/// Local delivery state; this is not a queue of work for the optimizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionRecord {
    /// Durable internal-entity/destination identity.
    pub key: ProjectionKey,
    /// Immutable identity bound before delivery.
    pub binding: ProjectionBinding,
    /// Last update with a durably recorded successful response.
    pub delivered: Option<ProjectionUpdate>,
    /// Write-ahead payload; replay is restricted to idempotent status assignment.
    pub pending: Option<ProjectionUpdate>,
    /// Projection-only failure. Never an internal work-state transition.
    pub last_error: Option<String>,
}

/// Result of reflecting one status onto a previously bound resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionReceipt {
    /// Existing external resource identity; absent in no-tracker mode.
    pub external_ref: Option<String>,
}

/// Delivery outcome, deliberately separate from optimizer acceptance/completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeliveryOutcome {
    /// Assignment succeeded and its completion was persisted.
    Delivered {
        /// Reflected internal revision.
        revision: u64,
        /// Previously bound external identity, if any.
        external_ref: Option<String>,
    },
    /// The exact revision/status was already durably delivered; no remote call.
    AlreadyDelivered {
        /// Existing delivered revision.
        revision: u64,
    },
    /// An older update cannot overwrite a newer pending or delivered revision.
    Superseded {
        /// Newer internal revision already recorded for delivery.
        current_revision: u64,
    },
    /// Projection failed; durable pending state remains available for retry.
    Deferred {
        /// Pending internal revision.
        revision: u64,
        /// Safe diagnostic, separate from domain failure.
        error: String,
    },
    /// There is no pending projection to retry.
    Idle,
}
