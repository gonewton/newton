//! Local dependency documents and the deterministic planner-facing query boundary.

use anyhow::{bail, Context, Result};
use newton_dependencies::{
    discover_cargo, ApprovedBaseline, BaselineApproval, BaselineDocument, CargoPackage,
    DependencyGraph, DependencyMap, DiscoveryReport, ImpactRequest, ImpactSequence, PlannedChange,
};
use serde::{de::DeserializeOwned, Serialize};
use std::{collections::BTreeMap, fs, io::Write, path::Path};

/// Canonical, unapproved map returned for human review after inspection/discovery.
#[derive(Debug, Serialize)]
pub struct DependencyReview {
    /// Version of this command's review envelope.
    pub schema_version: u32,
    /// Identity of all facts and unresolved inputs the human must review.
    pub map_fingerprint: String,
    /// Always true: inspecting/discovering never implicitly approves a map.
    pub approval_required: bool,
    /// Canonical facts; declarations and suggestions survive rediscovery.
    pub map: DependencyMap,
    /// Source-local facts and unresolved inputs, present after discovery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery_report: Option<DiscoveryReport>,
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes =
        fs::read(path).with_context(|| format!("read dependency input {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid dependency JSON in {}", path.display()))
}

fn read_graph(path: &Path) -> Result<DependencyGraph> {
    let mut value: serde_json::Value = read_json(path)?;
    // Reinspection/rediscovery intentionally discards any previous approval.
    if let Some(map) = value.get_mut("map") {
        value = map.take();
    }
    let map = serde_json::from_value(value).with_context(|| {
        format!(
            "{} must contain a DependencyMap or a document with map",
            path.display()
        )
    })?;
    Ok(DependencyGraph::new(map)?)
}

fn review(
    graph: DependencyGraph,
    discovery_report: Option<DiscoveryReport>,
) -> Result<DependencyReview> {
    Ok(DependencyReview {
        schema_version: 1,
        map_fingerprint: graph.fingerprint()?,
        approval_required: true,
        map: graph.map().clone(),
        discovery_report,
    })
}

/// Validate a local map and return its exact canonical review identity as JSON data.
pub fn inspect(map_path: &Path) -> Result<DependencyReview> {
    review(read_graph(map_path)?, None)
}

/// Read one real Cargo manifest and refresh only that source's detected facts.
///
/// `catalog_path` is a JSON array of explicit Cargo package/source identities.
/// Every package identity/version must agree with the supplied map. This command
/// performs no Cargo/Git/network execution and does not discover other ecosystems.
pub fn discover(
    map_path: &Path,
    manifest_path: &Path,
    catalog_path: &Path,
    owner_id: &str,
) -> Result<DependencyReview> {
    let graph = read_graph(map_path)?;
    let catalog: Vec<CargoPackage> = read_json(catalog_path)?;
    for package in &catalog {
        if graph.artifact(&package.artifact.id)? != &package.artifact {
            bail!("catalog version/identity for {} differs from the map; refresh and review the facts", package.artifact.id);
        }
    }
    let owners: Vec<_> = catalog
        .iter()
        .filter(|package| package.artifact.id == owner_id)
        .collect();
    let [owner] = owners.as_slice() else {
        bail!("owner {owner_id} must identify exactly one Cargo package in the catalog");
    };
    let manifest_path = fs::canonicalize(manifest_path)
        .with_context(|| format!("resolve manifest {}", manifest_path.display()))?;
    let contents = fs::read_to_string(&manifest_path)
        .with_context(|| format!("read manifest {}", manifest_path.display()))?;
    let report = discover_cargo(&contents, &manifest_path, owner, &catalog)?;
    review(graph.refresh_detected(report.clone())?, Some(report))
}

/// Validate a human-issued review record and durably create an immutable Baseline.
///
/// The local caller owns reviewer authorization. This function does not invent or
/// authenticate human identity, infer approval, or overwrite an existing file.
/// It is deliberately unavailable as an MCP/chat tool.
pub fn approve(map_path: &Path, review_path: &Path, output: &Path) -> Result<BaselineDocument> {
    let graph = read_graph(map_path)?;
    let approval: BaselineApproval = read_json(review_path)?;
    let document = ApprovedBaseline::approve(graph, approval)?.document();
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create baseline directory {}", parent.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temporary, &document)?;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)
        .with_context(|| {
            format!(
                "create baseline {}; existing baselines are immutable, choose a new path",
                output.display()
            )
        })?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(document)
}

/// Query a persisted human-approved Baseline without changing it or assigning versions.
pub fn impact(
    baseline_path: &Path,
    changed: &str,
    target: &str,
    changes_path: Option<&Path>,
) -> Result<ImpactSequence> {
    let baseline = ApprovedBaseline::from_document(read_json(baseline_path)?)?;
    let changes: BTreeMap<String, PlannedChange> = match changes_path {
        Some(path) => read_json(path)?,
        None => BTreeMap::new(),
    };
    Ok(baseline.impact_sequence(&ImpactRequest {
        changed: changed.into(),
        target: target.into(),
        changes,
    })?)
}
