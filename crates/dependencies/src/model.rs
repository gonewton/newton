use serde::{Deserialize, Serialize};

/// Addressable level in the portfolio hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    /// Portfolio product.
    Product,
    /// Product component.
    Component,
    /// Source repository.
    Repo,
    /// Independently identifiable crate or package; the planning unit.
    Module,
}

/// A project-owned version; Newton never invents a new value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scheme", content = "value", rename_all = "snake_case")]
pub enum Version {
    /// Semantic version with compatibility information.
    Semver(semver::Version),
    /// Calendar label. This initial implementation does not interpret its ordering.
    Calver(String),
    /// Exact commit identity, optionally annotated with a branch/tag.
    Commit {
        /// Commit object identity supplied by the project.
        sha: String,
        /// Optional human-readable branch/tag.
        ref_name: Option<String>,
    },
    /// Uninterpreted label without compatibility guarantees.
    Opaque(String),
}

/// Stable catalog identity and the current project-owned version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    /// Portfolio level.
    pub kind: ArtifactKind,
    /// Unique catalog identifier, independent of package name.
    pub id: String,
    /// Current version recorded in the reviewed map.
    pub version: Version,
}

/// Constraint placed by a dependent on the dependency it consumes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum VersionConstraint {
    /// Exact value in the target's version scheme.
    Exact(Version),
    /// Semver range; rejected for all other schemes during graph construction.
    Range(semver::VersionReq),
    /// Exact commit identity; rejected for non-commit targets.
    Pinned(String),
    /// No expressible version restriction. This makes no compatibility claim.
    Any,
}

/// Dependency use; all kinds participate unless a caller supplies a narrower map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    /// Production use.
    Runtime,
    /// Build-time use.
    Build,
    /// Development use.
    Dev,
    /// Explicitly declared test-time use.
    Test,
}

/// How a relationship became known. Confirmation is always derived.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Discovery {
    /// Read from an identified manifest or lockfile.
    Detected {
        /// Stable manifest identity, used to scope rediscovery.
        source: String,
    },
    /// Explicitly stated or promoted by an authorized person.
    Declared {
        /// Identity of the declaring person.
        by: String,
    },
    /// Unreviewed inference; excluded from automatic sequencing.
    Suggested {
        /// Method that proposed the edge.
        method: String,
        /// Informational confidence in [0, 1]; never grants confirmation.
        confidence: f32,
    },
}

/// Module-to-Module edge, directed from dependent to dependency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dependency {
    /// Dependent module identifier.
    pub from: String,
    /// Consumed module identifier.
    pub to: String,
    /// Use category.
    pub kind: DependencyKind,
    /// Requirement applied to `to`.
    pub constraint: VersionConstraint,
    /// Detection/declaration/suggestion metadata.
    pub discovery: Discovery,
}

impl Dependency {
    /// Whether the edge may participate in automatic sequencing.
    pub fn confirmed(&self) -> bool {
        matches!(
            self.discovery,
            Discovery::Detected { .. } | Discovery::Declared { .. }
        )
    }
}

/// Explicit ownership; not a dependency or an inferred relationship.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Membership {
    /// Product, Component, or Repo identifier.
    pub parent: String,
    /// Direct next-level child identifier.
    pub child: String,
}

/// Input that discovery could not resolve without guessing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DiscoveryIssue {
    /// Stable source-local identity, included in baseline approval.
    pub id: String,
    /// Manifest/discovery identity.
    pub source: String,
    /// Actionable reason that completeness cannot be assumed.
    pub message: String,
}

/// Refreshable facts from one explicit discovery source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryReport {
    /// Source identity shared by every detected edge and issue in this report.
    pub source: String,
    /// Confirmed edges actually resolved from machine-readable inputs.
    pub dependencies: Vec<Dependency>,
    /// Unresolved or unsupported inputs; empty does not prove cross-service completeness.
    pub issues: Vec<DiscoveryIssue>,
}

/// Serializable, unapproved dependency map. Construction validates its facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DependencyMap {
    /// All addressable artifacts, including target ownership anchors.
    pub artifacts: Vec<ArtifactRef>,
    /// Direct ownership relationships.
    pub memberships: Vec<Membership>,
    /// Relationship facts, including unconfirmed suggestions.
    pub dependencies: Vec<Dependency>,
    /// Unresolved discoveries retained for human review.
    pub issues: Vec<DiscoveryIssue>,
}

/// Compatibility information provided by a change owner or inferred from semver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilitySignal {
    /// Consumers may require source changes.
    Breaking,
    /// Mechanical re-pin/rebuild/release is sufficient according to the signal.
    NonBreaking,
    /// No justified compatibility claim.
    Unknown,
}

/// Work classification; it never removes a hop from propagation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffortClass {
    /// Mechanical propagation under a known non-breaking change.
    BumpOnly,
    /// Adaptation required by a breaking signal.
    Adapt,
    /// Uncertain and conservatively treated as adaptation work.
    Unknown,
}

impl EffortClass {
    /// Whether a planner must allow adaptation, including uncertain compatibility.
    pub fn requires_adaptation(self) -> bool {
        self != Self::BumpOnly
    }
}

impl From<CompatibilitySignal> for EffortClass {
    fn from(signal: CompatibilitySignal) -> Self {
        match signal {
            CompatibilitySignal::Breaking => Self::Adapt,
            CompatibilitySignal::NonBreaking => Self::BumpOnly,
            CompatibilitySignal::Unknown => Self::Unknown,
        }
    }
}

/// Planned facts supplied by a project's owner, never assigned by Newton.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlannedChange {
    /// Proposed version, if the project has already assigned one.
    pub version: Option<Version>,
    /// Explicit compatibility statement; useful for non-semver versions.
    pub compatibility: Option<CompatibilitySignal>,
}

impl PlannedChange {
    /// Determine compatibility without treating opaque schemes as semver.
    ///
    /// Explicit owner statements take precedence. Pre-release, unchanged, and
    /// decreasing semver values remain unknown. Pre-1.0 minor changes are breaking.
    pub fn signal(&self, previous: &Version) -> CompatibilitySignal {
        if let Some(signal) = self.compatibility {
            return signal;
        }
        match (previous, &self.version) {
            (Version::Semver(old), Some(Version::Semver(new)))
                if old.pre.is_empty() && new.pre.is_empty() && new.cmp_precedence(old).is_gt() =>
            {
                if new.major != old.major
                    || (old.major == 0 && new.minor != old.minor)
                    || (old.major == 0 && old.minor == 0 && new.patch != old.patch)
                {
                    CompatibilitySignal::Breaking
                } else {
                    CompatibilitySignal::NonBreaking
                }
            }
            _ => CompatibilitySignal::Unknown,
        }
    }
}
