use crate::{
    ArtifactKind, ArtifactRef, Dependency, DependencyError, DependencyKind, Discovery,
    DiscoveryIssue, DiscoveryReport, Version, VersionConstraint,
};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// Explicit Git selector, without treating a branch name as an exact commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum GitReference {
    /// Repository's default branch.
    Default,
    /// Named branch.
    Branch(String),
    /// Named tag.
    Tag(String),
    /// Cargo rev selector, which can be a full SHA or another Git revision.
    Rev(String),
}

/// Catalog package source used to resolve manifest facts without name guessing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PackageSource {
    /// Registry name as used by Cargo; the default is `crates-io`.
    Registry {
        /// Registry identity; mirrors `registry` in Cargo.toml.
        registry: String,
    },
    /// Directory containing a path dependency's Cargo.toml.
    Path {
        /// Caller-supplied absolute path. No filesystem or symlink resolution occurs.
        directory: PathBuf,
    },
    /// Exact Git URL and selector as declared by Cargo.
    Git {
        /// Repository URL, compared exactly rather than heuristically normalized.
        repository: String,
        /// Branch, tag, rev, or default branch.
        reference: GitReference,
    },
}

/// Caller-owned catalog mapping between a package/source and a Module identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CargoPackage {
    /// Existing Module identity and reviewed current version.
    pub artifact: ArtifactRef,
    /// Actual package name; dependency aliases are resolved using `package`.
    pub package: String,
    /// Explicit package source; ambiguous matches become unresolved diagnostics.
    pub source: PackageSource,
}

/// Discover direct Cargo.toml package dependencies from supplied text.
///
/// Supports runtime/build/dev dependencies, renamed packages, registry/path/Git
/// sources, and the conservative union of target-specific and optional edges.
/// Workspace inheritance, source overrides, missing/ambiguous catalog identities,
/// and unrepresentable compound version constraints are reported explicitly.
/// This function never opens files, follows symlinks, runs Cargo, or contacts a
/// registry. Callers scan each package manifest and supply canonical catalog paths.
pub fn discover_cargo(
    contents: &str,
    manifest_path: &Path,
    owner: &CargoPackage,
    catalog: &[CargoPackage],
) -> Result<DiscoveryReport, DependencyError> {
    if !manifest_path.is_absolute() {
        return Err(DependencyError::InvalidManifest(
            "manifest_path must be absolute for unambiguous path dependency resolution".into(),
        ));
    }
    if owner.artifact.kind != ArtifactKind::Module {
        return Err(DependencyError::InvalidManifest(
            "manifest owner must be a Module".into(),
        ));
    }
    let document: toml::Value = toml::from_str(contents)
        .map_err(|error| DependencyError::InvalidManifest(error.to_string()))?;
    let source = normalize_path(manifest_path).to_string_lossy().into_owned();
    let mut report = DiscoveryReport {
        source,
        dependencies: Vec::new(),
        issues: Vec::new(),
    };
    let Some(package) = document.get("package").and_then(toml::Value::as_table) else {
        issue(
            &mut report,
            "package",
            "no [package] table: virtual workspaces require scanning each member manifest",
        );
        return Ok(report);
    };
    if package.get("name").and_then(toml::Value::as_str) != Some(owner.package.as_str()) {
        return Err(DependencyError::InvalidManifest(format!(
            "[package].name does not match owner package {}",
            owner.package
        )));
    }
    if let Some(version) = package.get("version") {
        match version.as_str() {
            Some(value) => {
                let version = semver::Version::parse(value)
                    .map_err(|error| DependencyError::InvalidManifest(error.to_string()))?;
                if matches!(&owner.artifact.version, Version::Semver(current) if current != &version)
                {
                    return Err(DependencyError::InvalidManifest(
                        "owner's semver does not match [package].version; refresh the catalog"
                            .into(),
                    ));
                }
            }
            None => issue(
                &mut report,
                "package.version",
                "inherited package version requires workspace resolution before approval",
            ),
        }
    }
    if document.get("patch").is_some() || document.get("replace").is_some() {
        issue(
            &mut report,
            "source-overrides",
            "[patch]/[replace] source overrides are unsupported; no edges were guessed",
        );
        return Ok(report);
    }
    scan_sections(&document, "", manifest_path, owner, catalog, &mut report);
    if let Some(targets) = document.get("target").and_then(toml::Value::as_table) {
        for (target, config) in targets {
            scan_sections(
                config,
                &format!("target.{target}."),
                manifest_path,
                owner,
                catalog,
                &mut report,
            );
        }
    }
    report
        .dependencies
        .sort_by(|a, b| (&a.from, &a.to, &a.kind).cmp(&(&b.from, &b.to, &b.kind)));
    report.issues.sort();
    Ok(report)
}

fn scan_sections(
    document: &toml::Value,
    prefix: &str,
    path: &Path,
    owner: &CargoPackage,
    catalog: &[CargoPackage],
    report: &mut DiscoveryReport,
) {
    for (section, kind) in [
        ("dependencies", DependencyKind::Runtime),
        ("build-dependencies", DependencyKind::Build),
        ("dev-dependencies", DependencyKind::Dev),
    ] {
        let Some(value) = document.get(section) else {
            continue;
        };
        let location = format!("{prefix}{section}");
        let Some(entries) = value.as_table() else {
            issue(report, &location, "dependency section must be a table");
            continue;
        };
        for (alias, value) in entries {
            let location = format!("{location}.{alias}");
            match resolve_dependency(alias, value, path, catalog) {
                Ok((target, constraint)) => report.dependencies.push(Dependency {
                    from: owner.artifact.id.clone(),
                    to: target.artifact.id.clone(),
                    kind,
                    constraint,
                    discovery: Discovery::Detected {
                        source: report.source.clone(),
                    },
                }),
                Err(message) => issue(report, &location, &message),
            }
        }
    }
}

fn resolve_dependency<'a>(
    alias: &str,
    value: &toml::Value,
    manifest_path: &Path,
    catalog: &'a [CargoPackage],
) -> Result<(&'a CargoPackage, VersionConstraint), String> {
    let (name, requirement, source) = if let Some(requirement) = value.as_str() {
        (
            alias,
            Some(requirement),
            PackageSource::Registry {
                registry: "crates-io".into(),
            },
        )
    } else if let Some(table) = value.as_table() {
        if table.contains_key("workspace") {
            return Err(
                "workspace-inherited dependency needs workspace resolution; no edge guessed".into(),
            );
        }
        let string = |key: &str| -> Result<Option<&str>, String> {
            table
                .get(key)
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| format!("{key} must be a string"))
                })
                .transpose()
        };
        let name = string("package")?.unwrap_or(alias);
        let requirement = string("version")?;
        let path = string("path")?;
        let git = string("git")?;
        if path.is_some() && git.is_some() {
            return Err("dependency cannot select both path and git".into());
        }
        let source = if let Some(path) = path {
            PackageSource::Path {
                directory: normalize_path(
                    &manifest_path.parent().unwrap_or(Path::new("/")).join(path),
                ),
            }
        } else if let Some(repository) = git {
            let selectors: Vec<_> = ["rev", "branch", "tag"]
                .into_iter()
                .filter_map(|key| match string(key) {
                    Ok(Some(value)) => Some(Ok((key, value))),
                    Ok(None) => None,
                    Err(error) => Some(Err(error)),
                })
                .collect::<Result<_, _>>()?;
            if selectors.len() > 1 {
                return Err("git dependency has multiple revision selectors".into());
            }
            let reference = match selectors.first() {
                Some(("rev", value)) => GitReference::Rev((*value).into()),
                Some(("branch", value)) => GitReference::Branch((*value).into()),
                Some(("tag", value)) => GitReference::Tag((*value).into()),
                _ => GitReference::Default,
            };
            PackageSource::Git {
                repository: repository.into(),
                reference,
            }
        } else {
            if ["rev", "branch", "tag"]
                .iter()
                .any(|key| table.contains_key(*key))
            {
                return Err("Git revision selector without a git source".into());
            }
            PackageSource::Registry {
                registry: string("registry")?.unwrap_or("crates-io").into(),
            }
        };
        (name, requirement, source)
    } else {
        return Err("dependency must be a version string or table".into());
    };
    if matches!(source, PackageSource::Registry { .. }) && requirement.is_none() {
        return Err("registry dependency requires an explicit version requirement".into());
    }
    let candidates: Vec<_> = catalog
        .iter()
        .filter(|entry| entry.package == name && same_source(&entry.source, &source))
        .collect();
    let target = match candidates.as_slice() {
        [target] => *target,
        [] => {
            return Err(format!(
                "no catalog Module matches package {name} and source {source:?}"
            ))
        }
        _ => {
            return Err(format!(
                "ambiguous catalog Modules for package {name} and source {source:?}"
            ))
        }
    };
    if target.artifact.kind != ArtifactKind::Module {
        return Err(format!("catalog package {name} is not a Module"));
    }
    let pinned = match &source {
        PackageSource::Git {
            reference: GitReference::Rev(rev),
            ..
        } if [40, 64].contains(&rev.len()) && rev.bytes().all(|byte| byte.is_ascii_hexdigit()) => {
            Some(rev.clone())
        }
        _ => None,
    };
    let constraint = if let Some(requirement) = requirement {
        if !matches!(target.artifact.version, Version::Semver(_)) || pinned.is_some() {
            return Err("compound package-version/Git constraint is not representable in the catalog version scheme".into());
        }
        VersionConstraint::Range(
            semver::VersionReq::parse(requirement).map_err(|error| error.to_string())?,
        )
    } else if let Some(sha) = pinned {
        if !matches!(target.artifact.version, Version::Commit { .. }) {
            return Err("exact Git revision requires a commit-versioned catalog Module".into());
        }
        VersionConstraint::Pinned(sha)
    } else {
        VersionConstraint::Any
    };
    Ok((target, constraint))
}

fn same_source(a: &PackageSource, b: &PackageSource) -> bool {
    match (a, b) {
        (PackageSource::Path { directory: a }, PackageSource::Path { directory: b }) => {
            a.is_absolute() && b.is_absolute() && normalize_path(a) == normalize_path(b)
        }
        _ => a == b,
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

fn issue(report: &mut DiscoveryReport, location: &str, message: &str) {
    report.issues.push(DiscoveryIssue {
        id: format!("{}#{location}", report.source),
        source: report.source.clone(),
        message: message.into(),
    });
}
