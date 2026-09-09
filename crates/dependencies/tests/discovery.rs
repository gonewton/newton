use newton_dependencies::*;
use std::path::Path;

fn package(name: &str, source: PackageSource) -> CargoPackage {
    CargoPackage {
        artifact: ArtifactRef {
            kind: ArtifactKind::Module,
            id: name.into(),
            version: Version::Semver("1.0.0".parse().unwrap()),
        },
        package: name.into(),
        source,
    }
}

fn registry() -> PackageSource {
    PackageSource::Registry {
        registry: "crates-io".into(),
    }
}

fn owner() -> CargoPackage {
    package(
        "example-app",
        PackageSource::Path {
            directory: "/portfolio/app".into(),
        },
    )
}

#[test]
fn reads_real_cargo_fixture_with_explicit_sources_and_unresolved_inputs() {
    let mut git = package(
        "commit-lib",
        PackageSource::Git {
            repository: "https://example.test/commit-lib.git".into(),
            reference: GitReference::Rev("a".repeat(40)),
        },
    );
    git.artifact.version = Version::Commit {
        sha: "a".repeat(40),
        ref_name: None,
    };
    let catalog = [
        package("serde", registry()),
        package(
            "shared-lib",
            PackageSource::Path {
                directory: "/portfolio/shared-lib".into(),
            },
        ),
        git,
    ];
    let report = discover_cargo(
        include_str!("fixtures/Cargo.toml"),
        Path::new("/portfolio/app/Cargo.toml"),
        &owner(),
        &catalog,
    )
    .unwrap();
    assert_eq!(report.dependencies.len(), 5);
    assert_eq!(report.issues.len(), 2);
    assert!(report.dependencies.iter().all(Dependency::confirmed));
    assert!(report.dependencies.iter().all(|edge| matches!(&edge.discovery, Discovery::Detected { source } if source == "/portfolio/app/Cargo.toml")));
    assert!(report
        .dependencies
        .iter()
        .any(|edge| edge.to == "shared-lib"));
    assert!(report.dependencies.iter().any(
        |edge| matches!(&edge.constraint, VersionConstraint::Pinned(sha) if sha == &"a".repeat(40))
    ));
    assert!(report
        .dependencies
        .iter()
        .any(|edge| edge.kind == DependencyKind::Build));
    assert!(report
        .dependencies
        .iter()
        .any(|edge| edge.kind == DependencyKind::Dev));
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.message.contains("workspace")));
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.message.contains("no catalog")));
    let mut artifacts = vec![owner().artifact];
    artifacts.extend(catalog.iter().map(|entry| entry.artifact.clone()));
    let graph = DependencyGraph::new(DependencyMap {
        artifacts,
        memberships: vec![],
        dependencies: vec![],
        issues: vec![],
    })
    .unwrap();
    let graph = graph.refresh_detected(report.clone()).unwrap();
    assert_eq!(
        graph.fingerprint().unwrap(),
        graph
            .refresh_detected(report)
            .unwrap()
            .fingerprint()
            .unwrap()
    );
}

#[test]
fn ambiguous_catalog_identity_never_becomes_a_guessed_edge() {
    let mut second = package("serde", registry());
    second.artifact.id = "other-serde".into();
    let report = discover_cargo(
        "[package]\nname='example-app'\nversion='1.0.0'\n[dependencies]\nserde='1'",
        Path::new("/portfolio/app/Cargo.toml"),
        &owner(),
        &[package("serde", registry()), second],
    )
    .unwrap();
    assert!(report.dependencies.is_empty());
    assert_eq!(report.issues.len(), 1);
    assert!(report.issues[0].message.contains("ambiguous"));
}

#[test]
fn unsupported_overrides_and_virtual_workspaces_report_incompleteness() {
    for (manifest, expected) in [
        ("[workspace]\nmembers=['a','b']", "virtual workspaces"),
        ("[package]\nname='example-app'\n[dependencies]\nserde='1'\n[patch.crates-io]\nserde={path='../serde'}", "source overrides"),
    ] {
        let report = discover_cargo(manifest, Path::new("/portfolio/app/Cargo.toml"), &owner(), &[package("serde", registry())]).unwrap();
        assert!(report.dependencies.is_empty());
        assert!(report.issues.iter().any(|issue| issue.message.contains(expected)));
    }
}

#[test]
fn parser_fails_on_malformed_input_wrong_owner_or_relative_manifest_path() {
    let path = Path::new("/portfolio/app/Cargo.toml");
    assert!(discover_cargo("[broken", path, &owner(), &[]).is_err());
    assert!(discover_cargo("[package]\nname='wrong'", path, &owner(), &[]).is_err());
    assert!(discover_cargo(
        "[package]\nname='example-app'\nversion='2.0.0'",
        path,
        &owner(),
        &[]
    )
    .is_err());
    assert!(discover_cargo(
        "[package]\nname='example-app'",
        Path::new("Cargo.toml"),
        &owner(),
        &[]
    )
    .is_err());
}

#[test]
fn git_branch_does_not_fabricate_compatibility_or_an_exact_pin() {
    let mut git = package(
        "git-lib",
        PackageSource::Git {
            repository: "https://example.test/lib".into(),
            reference: GitReference::Branch("main".into()),
        },
    );
    git.artifact.version = Version::Commit {
        sha: "a".repeat(40),
        ref_name: Some("main".into()),
    };
    let manifest = "[package]\nname='example-app'\n[dependencies]\ngit-lib={git='https://example.test/lib',branch='main'}";
    let report = discover_cargo(
        manifest,
        Path::new("/portfolio/app/Cargo.toml"),
        &owner(),
        &[git],
    )
    .unwrap();
    assert!(report.issues.is_empty());
    assert_eq!(report.dependencies[0].constraint, VersionConstraint::Any);
}
