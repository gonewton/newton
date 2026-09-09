use crate::{
    ApprovedBaseline, ArtifactKind, ArtifactRef, CompatibilitySignal, DependencyError,
    DiscoveryIssue, EffortClass, PlannedChange,
};
use petgraph::{algo::kosaraju_scc, graph::DiGraph};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Planner request scoped to one changed Module and one portfolio target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactRequest {
    /// Changed Module identifier.
    pub changed: String,
    /// Product, Component, Repo, or Module whose dependency paths matter.
    pub target: String,
    /// Optional project-owned version/signal facts, keyed by Module identifier.
    /// Unknown downstream changes stay unknown; Newton does not assign versions.
    pub changes: BTreeMap<String, PlannedChange>,
}

/// One Module to re-release, including mechanically compatible propagation hops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactModule {
    /// Reviewed artifact identity and current version.
    pub artifact: ArtifactRef,
    /// Conservative effort based on incoming changed dependencies.
    pub effort: EffortClass,
}

/// A singleton release or an SCC requiring explicit co-release/cycle resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseGroup {
    /// Stable lexicographic member order; it is not an execution order within a cycle.
    pub members: Vec<ImpactModule>,
    /// True for multi-member SCCs and single-member self-dependencies.
    pub requires_co_release_resolution: bool,
}

/// Groups with no dependencies on one another; may be planned in parallel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseStage {
    /// Stable order for presentation, with no implied ordering within this stage.
    pub groups: Vec<ReleaseGroup>,
}

/// Deterministic propagation-to-target result, not proof of release execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactSequence {
    /// Identity of the human-approved map used by the planner.
    pub baseline_fingerprint: String,
    /// Changed Module.
    pub changed: String,
    /// Selected target.
    pub target: String,
    /// False when no confirmed dependency path connects this change to the target.
    pub reaches_target: bool,
    /// Dependency-first stages. An unreachable change produces no stages.
    pub stages: Vec<ReleaseStage>,
    /// IDs whose classification remains unknown and requires adaptation allowance.
    pub unknown_compatibility: Vec<String>,
    /// Acknowledged discovery limitations, retained so approval cannot hide them.
    pub discovery_limitations: Vec<DiscoveryIssue>,
}

impl ApprovedBaseline {
    /// Compute every confirmed hop carrying a change into the selected target.
    ///
    /// Intersects reverse reachability from `changed` with dependency reachability
    /// from target-owned Modules, then condenses SCCs into dependency-first stages.
    /// Constraints never prune traversal. Complexity is O(V + E), plus ordered-set
    /// and canonical output sorting costs; no recursive portfolio traversal is used.
    pub fn impact_sequence(
        &self,
        request: &ImpactRequest,
    ) -> Result<ImpactSequence, DependencyError> {
        let graph = self.graph();
        if graph.artifact(&request.changed)?.kind != ArtifactKind::Module {
            return Err(DependencyError::InvalidChange(request.changed.clone()));
        }
        for id in request.changes.keys() {
            if graph.artifact(id)?.kind != ArtifactKind::Module {
                return Err(DependencyError::InvalidChange(id.clone()));
            }
        }
        let target_modules = graph.target_modules(&request.target)?;
        let mut dependencies = BTreeMap::<String, BTreeSet<String>>::new();
        let mut dependents = BTreeMap::<String, BTreeSet<String>>::new();
        for edge in graph
            .map()
            .dependencies
            .iter()
            .filter(|edge| edge.confirmed())
        {
            dependencies
                .entry(edge.from.clone())
                .or_default()
                .insert(edge.to.clone());
            dependents
                .entry(edge.to.clone())
                .or_default()
                .insert(edge.from.clone());
        }
        let toward_target = reachable(target_modules, &dependencies);
        let impacted = reachable(BTreeSet::from([request.changed.clone()]), &dependents);
        let propagation: BTreeSet<String> =
            toward_target.intersection(&impacted).cloned().collect();
        let mut sequence = ImpactSequence {
            baseline_fingerprint: self.approval().map_fingerprint.clone(),
            changed: request.changed.clone(),
            target: request.target.clone(),
            reaches_target: propagation.contains(&request.changed),
            stages: Vec::new(),
            unknown_compatibility: Vec::new(),
            discovery_limitations: graph.map().issues.clone(),
        };
        if propagation.is_empty() {
            return Ok(sequence);
        }

        let mut directed = DiGraph::<String, ()>::new();
        let indices: BTreeMap<_, _> = propagation
            .iter()
            .map(|id| (id.clone(), directed.add_node(id.clone())))
            .collect();
        for from in &propagation {
            for to in dependencies.get(from).into_iter().flatten() {
                if propagation.contains(to) {
                    // Release the consumed dependency before its dependent.
                    directed.add_edge(indices[to], indices[from], ());
                }
            }
        }
        let mut components: Vec<Vec<String>> = kosaraju_scc(&directed)
            .into_iter()
            .map(|component| {
                let mut members: Vec<_> = component
                    .into_iter()
                    .map(|index| directed[index].clone())
                    .collect();
                members.sort();
                members
            })
            .collect();
        components.sort();
        let group_by_id: BTreeMap<_, _> = components
            .iter()
            .enumerate()
            .flat_map(|(index, group)| group.iter().map(move |id| (id.clone(), index)))
            .collect();
        let mut downstream = vec![BTreeSet::new(); components.len()];
        let mut indegree = vec![0usize; components.len()];
        for from in &propagation {
            for to in dependencies.get(from).into_iter().flatten() {
                if let Some(&dependency_group) = group_by_id.get(to) {
                    let dependent_group = group_by_id[from];
                    if dependency_group != dependent_group
                        && downstream[dependency_group].insert(dependent_group)
                    {
                        indegree[dependent_group] += 1;
                    }
                }
            }
        }
        let signal = |id: &str| {
            request
                .changes
                .get(id)
                .map(|change| {
                    change.signal(&graph.artifact(id).expect("validated identity").version)
                })
                .unwrap_or(CompatibilitySignal::Unknown)
        };
        let mut ready: BTreeSet<usize> = indegree
            .iter()
            .enumerate()
            .filter_map(|(index, count)| (*count == 0).then_some(index))
            .collect();
        while !ready.is_empty() {
            let current = std::mem::take(&mut ready);
            let mut stage = ReleaseStage { groups: Vec::new() };
            for index in current {
                let ids = &components[index];
                let mut members = Vec::new();
                for id in ids {
                    let incoming: Vec<_> = dependencies
                        .get(id)
                        .into_iter()
                        .flatten()
                        .filter(|dependency| propagation.contains(*dependency))
                        .map(|dependency| signal(dependency))
                        .collect();
                    let compatibility = if incoming.is_empty() {
                        signal(id)
                    } else if incoming.contains(&CompatibilitySignal::Breaking) {
                        CompatibilitySignal::Breaking
                    } else if incoming.contains(&CompatibilitySignal::Unknown) {
                        CompatibilitySignal::Unknown
                    } else {
                        CompatibilitySignal::NonBreaking
                    };
                    let effort = EffortClass::from(compatibility);
                    if effort == EffortClass::Unknown {
                        sequence.unknown_compatibility.push(id.clone());
                    }
                    members.push(ImpactModule {
                        artifact: graph.artifact(id)?.clone(),
                        effort,
                    });
                }
                let self_cycle = ids.len() == 1
                    && dependencies
                        .get(&ids[0])
                        .is_some_and(|deps| deps.contains(&ids[0]));
                stage.groups.push(ReleaseGroup {
                    members,
                    requires_co_release_resolution: ids.len() > 1 || self_cycle,
                });
                for &dependent in &downstream[index] {
                    indegree[dependent] -= 1;
                    if indegree[dependent] == 0 {
                        ready.insert(dependent);
                    }
                }
            }
            sequence.stages.push(stage);
        }
        sequence.unknown_compatibility.sort();
        Ok(sequence)
    }
}

fn reachable(
    initial: BTreeSet<String>,
    neighbors: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut visited = BTreeSet::new();
    let mut pending: Vec<String> = initial.into_iter().collect();
    while let Some(id) = pending.pop() {
        if visited.insert(id.clone()) {
            pending.extend(neighbors.get(&id).into_iter().flatten().cloned());
        }
    }
    visited
}
