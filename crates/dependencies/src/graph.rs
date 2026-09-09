use crate::{
    ArtifactKind, ArtifactRef, DependencyError, DependencyMap, Discovery, DiscoveryReport, Version,
    VersionConstraint,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Validated, canonical dependency facts. A graph is not yet a planning Baseline.
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    map: DependencyMap,
    artifacts: BTreeMap<String, ArtifactRef>,
}

impl DependencyGraph {
    /// Validate and canonicalize a draft map without approving its completeness.
    ///
    /// Invalid endpoint identities and inexpressible version constraints fail
    /// closed, including on suggestions. Construction is O((V + E) log(V + E)).
    pub fn new(mut map: DependencyMap) -> Result<Self, DependencyError> {
        let mut artifacts = BTreeMap::new();
        for artifact in &map.artifacts {
            if artifact.id.trim().is_empty()
                || artifacts
                    .insert(artifact.id.clone(), artifact.clone())
                    .is_some()
            {
                return Err(DependencyError::InvalidIdentity(artifact.id.clone()));
            }
            validate_version(&artifact.version)?;
        }
        let mut parents = BTreeMap::new();
        for membership in &map.memberships {
            let parent = get_artifact(&artifacts, &membership.parent)?;
            let child = get_artifact(&artifacts, &membership.child)?;
            let valid = matches!(
                (parent.kind, child.kind),
                (ArtifactKind::Product, ArtifactKind::Component)
                    | (ArtifactKind::Component, ArtifactKind::Repo)
                    | (ArtifactKind::Repo, ArtifactKind::Module)
            );
            if !valid
                || parents
                    .insert(child.id.clone(), parent.id.clone())
                    .is_some_and(|old| old != parent.id)
            {
                return Err(DependencyError::InvalidMembership(format!(
                    "{} -> {}",
                    parent.id, child.id
                )));
            }
        }
        for edge in &map.dependencies {
            let from = get_artifact(&artifacts, &edge.from)?;
            let to = get_artifact(&artifacts, &edge.to)?;
            if from.kind != ArtifactKind::Module || to.kind != ArtifactKind::Module {
                return Err(DependencyError::InvalidDependency(format!(
                    "{} -> {}: this version supports module-to-module edges only",
                    from.id, to.id
                )));
            }
            validate_constraint(&edge.constraint, &to.version)?;
            let discovery_valid = match &edge.discovery {
                Discovery::Detected { source } => !source.trim().is_empty(),
                Discovery::Declared { by } => !by.trim().is_empty(),
                Discovery::Suggested { method, confidence } => {
                    !method.trim().is_empty() && (0.0..=1.0).contains(confidence)
                }
            };
            if !discovery_valid {
                return Err(DependencyError::InvalidDependency(format!(
                    "{} -> {} has invalid Discovery metadata",
                    edge.from, edge.to
                )));
            }
        }
        let mut issues = BTreeSet::new();
        for issue in &map.issues {
            if issue.id.trim().is_empty()
                || issue.source.trim().is_empty()
                || issue.message.trim().is_empty()
                || !issues.insert(issue.id.clone())
            {
                return Err(DependencyError::InvalidDependency(format!(
                    "invalid or duplicate discovery issue: {}",
                    issue.id
                )));
            }
        }
        map.artifacts.sort_by(|a, b| a.id.cmp(&b.id));
        map.memberships.sort();
        map.memberships.dedup();
        map.issues.sort();
        let mut edges = map
            .dependencies
            .into_iter()
            .map(|edge| Ok((serde_json::to_string(&edge)?, edge)))
            .collect::<Result<Vec<_>, DependencyError>>()?;
        edges.sort_by(|a, b| a.0.cmp(&b.0));
        edges.dedup_by(|a, b| a.0 == b.0);
        map.dependencies = edges.into_iter().map(|(_, edge)| edge).collect();
        Ok(Self { map, artifacts })
    }

    /// Canonical serializable facts, including unresolved inputs and suggestions.
    pub fn map(&self) -> &DependencyMap {
        &self.map
    }

    /// Content identity of all reviewed facts; independent of input ordering.
    pub fn fingerprint(&self) -> Result<String, DependencyError> {
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&self.map)?)))
    }

    /// Look up an artifact without guessing from its package name.
    pub fn artifact(&self, id: &str) -> Result<&ArtifactRef, DependencyError> {
        get_artifact(&self.artifacts, id)
    }

    /// Refresh one manifest's detected facts, preserving declarations/suggestions.
    ///
    /// Returns an unapproved graph even when invoked on a previously approved
    /// baseline's graph. Approval is never inherited across rediscovery.
    pub fn refresh_detected(&self, report: DiscoveryReport) -> Result<Self, DependencyError> {
        if report.source.trim().is_empty()
            || report.dependencies.iter().any(|edge| {
                !matches!(&edge.discovery, Discovery::Detected { source } if source == &report.source)
            })
            || report.issues.iter().any(|issue| issue.source != report.source)
        {
            return Err(DependencyError::InvalidDependency(
                "discovery report contains facts from a different source or non-detected edges"
                    .into(),
            ));
        }
        let mut map = self.map.clone();
        map.dependencies.retain(|edge| {
            !matches!(&edge.discovery, Discovery::Detected { source } if source == &report.source)
        });
        map.issues.retain(|issue| issue.source != report.source);
        map.dependencies.extend(report.dependencies);
        map.issues.extend(report.issues);
        Self::new(map)
    }

    pub(crate) fn target_modules(&self, target: &str) -> Result<BTreeSet<String>, DependencyError> {
        self.artifact(target)?;
        let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for membership in &self.map.memberships {
            children
                .entry(&membership.parent)
                .or_default()
                .push(&membership.child);
        }
        let mut pending = vec![target];
        let mut modules = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if self.artifacts[id].kind == ArtifactKind::Module {
                modules.insert(id.to_owned());
            }
            if let Some(direct) = children.get(id) {
                pending.extend(direct.iter().copied());
            }
        }
        Ok(modules)
    }
}

fn get_artifact<'a>(
    artifacts: &'a BTreeMap<String, ArtifactRef>,
    id: &str,
) -> Result<&'a ArtifactRef, DependencyError> {
    artifacts
        .get(id)
        .ok_or_else(|| DependencyError::UnknownArtifact(id.into()))
}

fn validate_version(version: &Version) -> Result<(), DependencyError> {
    let valid = match version {
        Version::Semver(_) => true,
        Version::Commit { sha, .. } => !sha.trim().is_empty(),
        Version::Calver(label) | Version::Opaque(label) => !label.trim().is_empty(),
    };
    if valid {
        Ok(())
    } else {
        Err(DependencyError::InvalidDependency(
            "empty version value".into(),
        ))
    }
}

fn validate_constraint(
    constraint: &VersionConstraint,
    version: &Version,
) -> Result<(), DependencyError> {
    let valid = match constraint {
        VersionConstraint::Any => true,
        VersionConstraint::Range(_) => matches!(version, Version::Semver(_)),
        VersionConstraint::Pinned(sha) => {
            !sha.trim().is_empty() && matches!(version, Version::Commit { .. })
        }
        VersionConstraint::Exact(exact) => {
            validate_version(exact)?;
            std::mem::discriminant(exact) == std::mem::discriminant(version)
        }
    };
    if valid {
        Ok(())
    } else {
        Err(DependencyError::InvalidDependency(format!(
            "constraint {constraint:?} cannot be evaluated against {version:?}"
        )))
    }
}
