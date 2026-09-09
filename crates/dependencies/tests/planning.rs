use newton_dependencies::*;
use std::collections::{BTreeMap, BTreeSet};

fn version(value: &str) -> Version {
    Version::Semver(value.parse().unwrap())
}

fn artifact(id: &str, kind: ArtifactKind) -> ArtifactRef {
    ArtifactRef {
        id: id.into(),
        kind,
        version: version("1.0.0"),
    }
}

fn edge(from: &str, to: &str) -> Dependency {
    Dependency {
        from: from.into(),
        to: to.into(),
        kind: DependencyKind::Runtime,
        constraint: VersionConstraint::Range("^1".parse().unwrap()),
        discovery: Discovery::Declared {
            by: "maintainer".into(),
        },
    }
}

fn map() -> DependencyMap {
    let mut artifacts: Vec<_> = ["base", "middle", "app", "other"]
        .into_iter()
        .map(|id| artifact(id, ArtifactKind::Module))
        .collect();
    artifacts.extend([
        artifact("product", ArtifactKind::Product),
        artifact("component", ArtifactKind::Component),
        artifact("repo", ArtifactKind::Repo),
    ]);
    DependencyMap {
        artifacts,
        memberships: [
            ("product", "component"),
            ("component", "repo"),
            ("repo", "app"),
        ]
        .into_iter()
        .map(|(parent, child)| Membership {
            parent: parent.into(),
            child: child.into(),
        })
        .collect(),
        dependencies: vec![
            edge("middle", "base"),
            edge("app", "middle"),
            edge("other", "base"),
        ],
        issues: vec![],
    }
}

fn approval(graph: &DependencyGraph) -> BaselineApproval {
    BaselineApproval {
        map_fingerprint: graph.fingerprint().unwrap(),
        reviewed_by: "human@example.test".into(),
        reviewed_at: "2026-09-09T00:00:00Z".into(),
        completeness_statement: "Reviewed package and cross-service relationships for this product"
            .into(),
        acknowledged_issues: graph
            .map()
            .issues
            .iter()
            .map(|issue| issue.id.clone())
            .collect(),
    }
}

fn approve(map: DependencyMap) -> ApprovedBaseline {
    let graph = DependencyGraph::new(map).unwrap();
    let approval = approval(&graph);
    ApprovedBaseline::approve(graph, approval).unwrap()
}

fn request() -> ImpactRequest {
    ImpactRequest {
        changed: "base".into(),
        target: "product".into(),
        changes: ["base", "middle", "app"]
            .into_iter()
            .map(|id| {
                (
                    id.into(),
                    PlannedChange {
                        version: Some(version("1.1.0")),
                        compatibility: None,
                    },
                )
            })
            .collect(),
    }
}

fn stage_ids(sequence: &ImpactSequence) -> Vec<Vec<Vec<&str>>> {
    sequence
        .stages
        .iter()
        .map(|stage| {
            stage
                .groups
                .iter()
                .map(|group| {
                    group
                        .members
                        .iter()
                        .map(|member| member.artifact.id.as_str())
                        .collect()
                })
                .collect()
        })
        .collect()
}

#[test]
fn public_planner_api_preserves_compatible_hops_and_excludes_unrelated_product() {
    let baseline = approve(map());
    let sequence = baseline.impact_sequence(&request()).unwrap();
    assert!(sequence.reaches_target);
    assert_eq!(
        stage_ids(&sequence),
        vec![vec![vec!["base"]], vec![vec!["middle"]], vec![vec!["app"]]]
    );
    assert!(sequence
        .stages
        .iter()
        .flat_map(|stage| &stage.groups)
        .flat_map(|group| &group.members)
        .all(|member| member.effort == EffortClass::BumpOnly));
    assert_eq!(
        sequence.baseline_fingerprint,
        baseline.approval().map_fingerprint
    );
    assert!(sequence.unknown_compatibility.is_empty());
    let wire = serde_json::to_string(&sequence).unwrap();
    assert_eq!(sequence, serde_json::from_str(&wire).unwrap());
}

#[test]
fn suggestions_never_supply_missing_path_even_with_maximum_confidence() {
    let mut draft = map();
    draft.dependencies[0].discovery = Discovery::Suggested {
        method: "agent".into(),
        confidence: 1.0,
    };
    assert!(!draft.dependencies[0].confirmed());
    let sequence = approve(draft.clone()).impact_sequence(&request()).unwrap();
    assert!(!sequence.reaches_target);
    assert!(sequence.stages.is_empty());
    draft.dependencies[0].discovery = Discovery::Declared {
        by: "authorized reviewer".into(),
    };
    assert!(
        approve(draft)
            .impact_sequence(&request())
            .unwrap()
            .reaches_target
    );
}

#[test]
fn graph_and_sequence_are_stable_under_input_permutation_and_duplicate_edges() {
    let first = approve(map());
    let mut reversed = map();
    reversed.artifacts.reverse();
    reversed.memberships.reverse();
    reversed.dependencies.reverse();
    reversed.dependencies.push(reversed.dependencies[0].clone());
    let second = approve(reversed);
    assert_eq!(
        first.graph().fingerprint().unwrap(),
        second.graph().fingerprint().unwrap()
    );
    assert_eq!(
        first.impact_sequence(&request()).unwrap(),
        second.impact_sequence(&request()).unwrap()
    );
}

#[test]
fn scc_is_co_release_group_and_never_an_arbitrary_linear_order() {
    let mut draft = map();
    draft.dependencies.push(edge("base", "middle"));
    let sequence = approve(draft).impact_sequence(&request()).unwrap();
    assert_eq!(
        stage_ids(&sequence),
        vec![vec![vec!["base", "middle"]], vec![vec!["app"]]]
    );
    assert!(sequence.stages[0].groups[0].requires_co_release_resolution);
    assert!(!sequence.stages[1].groups[0].requires_co_release_resolution);
}

#[test]
fn self_dependency_is_also_an_explicit_co_release_group() {
    let mut draft = map();
    draft.dependencies.push(edge("base", "base"));
    let sequence = approve(draft).impact_sequence(&request()).unwrap();
    assert!(sequence.stages[0].groups[0].requires_co_release_resolution);
}

#[test]
fn independent_branches_share_a_deterministic_parallel_stage() {
    let mut draft = map();
    draft.dependencies.push(edge("app", "other"));
    let sequence = approve(draft).impact_sequence(&request()).unwrap();
    assert_eq!(
        stage_ids(&sequence),
        vec![
            vec![vec!["base"]],
            vec![vec!["middle"], vec!["other"]],
            vec![vec!["app"]]
        ]
    );
}

#[test]
fn unassigned_downstream_versions_are_visibly_unknown_not_invented() {
    let baseline = approve(map());
    let mut request = request();
    request.changes.remove("middle");
    let sequence = baseline.impact_sequence(&request).unwrap();
    assert_eq!(sequence.unknown_compatibility, vec!["app"]);
    let effort = sequence.stages[2].groups[0].members[0].effort;
    assert_eq!(effort, EffortClass::Unknown);
    assert!(effort.requires_adaptation());
    assert!(!EffortClass::BumpOnly.requires_adaptation());
    assert!(EffortClass::Adapt.requires_adaptation());
}

#[test]
fn commit_calver_and_opaque_changes_require_explicit_compatibility() {
    for old in [
        Version::Commit {
            sha: "abc".into(),
            ref_name: None,
        },
        Version::Calver("2026.01".into()),
        Version::Opaque("stable".into()),
    ] {
        let change = PlannedChange {
            version: Some(old.clone()),
            compatibility: None,
        };
        assert_eq!(change.signal(&old), CompatibilitySignal::Unknown);
        assert_eq!(
            PlannedChange {
                compatibility: Some(CompatibilitySignal::NonBreaking),
                ..change
            }
            .signal(&old),
            CompatibilitySignal::NonBreaking
        );
    }
}

#[test]
fn semver_major_and_pre_one_minor_changes_are_breaking() {
    for (old, new, expected) in [
        ("1.0.0", "2.0.0", CompatibilitySignal::Breaking),
        ("0.2.0", "0.3.0", CompatibilitySignal::Breaking),
        ("1.0.0", "1.0.1", CompatibilitySignal::NonBreaking),
        ("1.0.0", "1.0.0+build", CompatibilitySignal::Unknown),
        ("2.0.0", "1.0.0", CompatibilitySignal::Unknown),
        ("1.0.0-beta.1", "1.0.0", CompatibilitySignal::Unknown),
    ] {
        assert_eq!(
            PlannedChange {
                version: Some(version(new)),
                compatibility: None
            }
            .signal(&version(old)),
            expected
        );
    }
}

#[test]
fn version_constraints_fail_closed_for_wrong_schemes() {
    let mut draft = map();
    draft
        .artifacts
        .iter_mut()
        .find(|artifact| artifact.id == "base")
        .unwrap()
        .version = Version::Commit {
        sha: "abc".into(),
        ref_name: None,
    };
    assert!(matches!(
        DependencyGraph::new(draft.clone()),
        Err(DependencyError::InvalidDependency(_))
    ));
    for edge in &mut draft.dependencies {
        if edge.to == "base" {
            edge.constraint = VersionConstraint::Pinned("abc".into());
        }
    }
    assert!(DependencyGraph::new(draft).is_ok());
    let mut draft = map();
    draft.dependencies[0].constraint = VersionConstraint::Pinned("abc".into());
    assert!(DependencyGraph::new(draft).is_err());
    let mut draft = map();
    draft.dependencies[0].constraint = VersionConstraint::Exact(Version::Opaque("stable".into()));
    assert!(DependencyGraph::new(draft).is_err());
}

#[test]
fn rediscovery_preserves_human_declarations_suggestions_and_other_sources() {
    let mut draft = map();
    draft.dependencies.push(Dependency {
        discovery: Discovery::Suggested {
            method: "analyzer".into(),
            confidence: 0.6,
        },
        ..edge("other", "middle")
    });
    draft.dependencies.push(Dependency {
        discovery: Discovery::Detected {
            source: "old/Cargo.toml".into(),
        },
        ..edge("app", "base")
    });
    draft.dependencies.push(Dependency {
        discovery: Discovery::Detected {
            source: "other/Cargo.toml".into(),
        },
        ..edge("other", "app")
    });
    let graph = DependencyGraph::new(draft).unwrap();
    let report = DiscoveryReport {
        source: "old/Cargo.toml".into(),
        dependencies: vec![],
        issues: vec![],
    };
    let refreshed = graph.refresh_detected(report.clone()).unwrap();
    assert_eq!(refreshed.map().dependencies.len(), 5);
    assert_eq!(
        refreshed
            .map()
            .dependencies
            .iter()
            .filter(|edge| matches!(edge.discovery, Discovery::Declared { .. }))
            .count(),
        3
    );
    assert!(refreshed
        .map()
        .dependencies
        .iter()
        .any(|edge| matches!(edge.discovery, Discovery::Suggested { .. })));
    assert!(refreshed.map().dependencies.iter().any(|edge| matches!(&edge.discovery, Discovery::Detected { source } if source == "other/Cargo.toml")));
    assert_eq!(
        refreshed.fingerprint().unwrap(),
        refreshed
            .refresh_detected(report)
            .unwrap()
            .fingerprint()
            .unwrap()
    );
    assert!(ApprovedBaseline::approve(refreshed, approval(&graph)).is_err());
}

#[test]
fn malformed_report_cannot_overwrite_declared_facts() {
    let graph = DependencyGraph::new(map()).unwrap();
    let report = DiscoveryReport {
        source: "Cargo.toml".into(),
        dependencies: vec![edge("app", "base")],
        issues: vec![],
    };
    assert!(graph.refresh_detected(report).is_err());
}

#[test]
fn approval_requires_exact_reviewed_map_and_acknowledged_unknown_inputs() {
    let mut draft = map();
    draft.issues.push(DiscoveryIssue {
        id: "manifest#missing".into(),
        source: "manifest".into(),
        message: "Resolve missing package".into(),
    });
    let graph = DependencyGraph::new(draft).unwrap();
    let mut approved = approval(&graph);
    approved.acknowledged_issues.clear();
    assert!(ApprovedBaseline::approve(graph.clone(), approved).is_err());
    let baseline = ApprovedBaseline::approve(graph.clone(), approval(&graph)).unwrap();
    assert_eq!(
        baseline
            .impact_sequence(&request())
            .unwrap()
            .discovery_limitations
            .len(),
        1
    );
    let wire = serde_json::to_string(&baseline.document()).unwrap();
    assert!(ApprovedBaseline::from_document(serde_json::from_str(&wire).unwrap()).is_ok());
    let mut tampered = baseline.document();
    tampered.map.dependencies.pop();
    assert!(ApprovedBaseline::from_document(tampered).is_err());
    let mut approved = approval(&graph);
    approved.reviewed_by.clear();
    assert!(ApprovedBaseline::approve(graph, approved).is_err());
}

#[test]
fn invalid_hierarchy_identity_or_discovery_fails_before_planning() {
    let mut draft = map();
    draft.artifacts.push(draft.artifacts[0].clone());
    assert!(DependencyGraph::new(draft).is_err());
    let mut draft = map();
    draft.memberships.push(Membership {
        parent: "base".into(),
        child: "product".into(),
    });
    assert!(DependencyGraph::new(draft).is_err());
    let mut draft = map();
    draft.dependencies[0].to = "missing".into();
    assert!(DependencyGraph::new(draft).is_err());
    let mut draft = map();
    draft.dependencies[0].discovery = Discovery::Suggested {
        method: "agent".into(),
        confidence: f32::NAN,
    };
    assert!(DependencyGraph::new(draft).is_err());
    let baseline = approve(map());
    let mut invalid = request();
    invalid.changed = "product".into();
    assert!(baseline.impact_sequence(&invalid).is_err());
    invalid = request();
    invalid.target = "missing".into();
    assert!(baseline.impact_sequence(&invalid).is_err());
}

#[test]
fn plans_a_portfolio_with_more_than_one_hundred_dependency_hops() {
    let ids: Vec<_> = (0..150).map(|index| format!("module-{index:03}")).collect();
    let draft = DependencyMap {
        artifacts: ids
            .iter()
            .map(|id| artifact(id, ArtifactKind::Module))
            .collect(),
        memberships: vec![],
        dependencies: ids
            .windows(2)
            .map(|pair| edge(&pair[1], &pair[0]))
            .collect(),
        issues: vec![],
    };
    let sequence = approve(draft)
        .impact_sequence(&ImpactRequest {
            changed: ids[0].clone(),
            target: ids[149].clone(),
            changes: BTreeMap::new(),
        })
        .unwrap();
    assert_eq!(sequence.stages.len(), 150);
    assert_eq!(sequence.unknown_compatibility.len(), 150);
    assert_eq!(
        sequence
            .stages
            .iter()
            .flat_map(|stage| &stage.groups)
            .flat_map(|group| &group.members)
            .map(|member| member.artifact.id.clone())
            .collect::<BTreeSet<_>>(),
        ids.into_iter().collect()
    );
}
