//! Content-pinned, explicitly declared local inputs. This is not a host sandbox.

use anyhow::{Context, Result};
use newton_core::workflow::schema::WorkflowDocument;
use newton_core::workflow::state::compute_sha256_hex;
use newton_types::optimization::BoundOptimizationDefinition;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Manifest {
    pub definition_sha256: String,
    pub files: BTreeMap<String, String>,
}

pub(super) struct Prepared {
    pub manifest: Manifest,
    files: BTreeMap<String, (Vec<u8>, fs::Permissions)>,
}

/// Verified execution inputs retained in process memory for the whole run.
#[derive(Clone)]
pub(super) struct Runtime {
    workflows: BTreeMap<String, WorkflowDocument>,
    assets: serde_json::Value,
}

fn references(binding: &BoundOptimizationDefinition) -> BTreeSet<String> {
    binding
        .definition
        .workflows
        .values()
        .cloned()
        .chain(binding.definition.assets.iter().cloned())
        .chain(
            binding
                .definition
                .requirements
                .evaluators
                .values()
                .map(|e| e.workflow.clone()),
        )
        .collect()
}

fn local_path(root: &Path, reference: &str) -> Result<PathBuf> {
    let path = Path::new(reference);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        anyhow::bail!("snapshot reference must be a relative path without traversal: {reference}");
    }
    let resolved = root
        .join(path)
        .canonicalize()
        .with_context(|| format!("resolve declared snapshot input {reference}"))?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        anyhow::bail!("snapshot input must be a file inside the definition directory: {reference}");
    }
    Ok(resolved)
}

impl Prepared {
    /// Read each declared file once. Do not copy an entire project or secret tree.
    pub fn read(binding: &BoundOptimizationDefinition, root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let mut files = BTreeMap::new();
        let mut hashes = BTreeMap::new();
        for reference in references(binding) {
            let path = local_path(&root, &reference)?;
            let bytes = fs::read(&path)?;
            hashes.insert(reference.clone(), compute_sha256_hex(&bytes));
            files.insert(reference, (bytes, fs::metadata(path)?.permissions()));
        }
        Ok(Self {
            manifest: Manifest {
                definition_sha256: compute_sha256_hex(&serde_json::to_vec(&binding.definition)?),
                files: hashes,
            },
            files,
        })
    }

    /// Fail if preflight or another writer changed any checked source content.
    pub fn verify_source(&self, binding: &BoundOptimizationDefinition, root: &Path) -> Result<()> {
        self.manifest.verify(binding, root)
    }

    /// Compile exactly the bytes read for the manifest, before any workflow runs.
    pub fn runtime(&self, binding: &BoundOptimizationDefinition) -> Result<Runtime> {
        Runtime::from_files(binding, &self.manifest, &self.files)
    }

    /// Publish a complete snapshot atomically before creating a run or candidate.
    pub fn persist(&self, state_dir: &Path, run_id: &str) -> Result<PathBuf> {
        let runs = state_dir.join("optimize");
        fs::create_dir_all(&runs)?;
        let staging = tempfile::Builder::new()
            .prefix(".definition-")
            .tempdir_in(&runs)?;
        let definition = staging.path().join("definition");
        fs::create_dir(&definition)?;
        for (reference, (bytes, permissions)) in &self.files {
            let path = definition.join(reference);
            fs::create_dir_all(path.parent().context("snapshot input parent")?)?;
            fs::write(&path, bytes)?;
            let mut permissions = permissions.clone();
            permissions.set_readonly(true);
            fs::set_permissions(path, permissions)?;
        }
        fs::write(
            staging.path().join("definition-manifest.json"),
            serde_json::to_vec_pretty(&self.manifest)?,
        )?;
        let destination = runs.join(run_id);
        if destination.exists() {
            anyhow::bail!("Optimize Run {run_id} already exists; use explicit resume");
        }
        fs::rename(staging.path(), &destination)
            .context("publish Optimization Definition snapshot")?;
        Ok(destination.join("definition").canonicalize()?)
    }
}

impl Runtime {
    fn from_files(
        binding: &BoundOptimizationDefinition,
        manifest: &Manifest,
        files: &BTreeMap<String, (Vec<u8>, fs::Permissions)>,
    ) -> Result<Self> {
        manifest.verify_identity(binding)?;
        let workflow_references = binding
            .definition
            .workflows
            .values()
            .chain(
                binding
                    .definition
                    .requirements
                    .evaluators
                    .values()
                    .map(|e| &e.workflow),
            )
            .chain(
                binding
                    .requirements
                    .requirements
                    .evaluators
                    .values()
                    .map(|e| &e.workflow),
            )
            .collect::<BTreeSet<_>>();
        let mut workflows = BTreeMap::new();
        for reference in workflow_references {
            let (bytes, _) = files
                .get(reference)
                .with_context(|| format!("immutable workflow is missing: {reference}"))?;
            let source = std::str::from_utf8(bytes)
                .with_context(|| format!("workflow must be UTF-8 YAML: {reference}"))?;
            let (document, _) = newton_core::workflow::loader::load_and_lint_workflow_source(
                source,
                Path::new(reference),
            )
            .map_err(|e| anyhow::anyhow!("{reference}: {}: {}", e.code, e.message))?;
            workflows.insert(reference.clone(), document);
        }
        let mut assets = serde_json::Map::new();
        for reference in &binding.definition.assets {
            let (bytes, _) = files
                .get(reference)
                .with_context(|| format!("immutable asset is missing: {reference}"))?;
            let source = std::str::from_utf8(bytes).with_context(|| {
                format!("optimization asset must be UTF-8 for in-memory dispatch: {reference}")
            })?;
            assets.insert(
                reference.clone(),
                serde_json::Value::String(source.to_owned()),
            );
        }
        Ok(Self {
            workflows,
            assets: serde_json::Value::Object(assets),
        })
    }

    pub fn workflow(&self, reference: &str) -> Result<WorkflowDocument> {
        self.workflows
            .get(reference)
            .cloned()
            .with_context(|| format!("immutable workflow is missing: {reference}"))
    }

    pub fn assets(&self) -> serde_json::Value {
        self.assets.clone()
    }

    pub fn asset(&self, reference: &str) -> Result<&str> {
        self.assets
            .get(reference)
            .and_then(serde_json::Value::as_str)
            .with_context(|| format!("immutable asset is missing: {reference}"))
    }

    fn load(
        binding: &BoundOptimizationDefinition,
        manifest: &Manifest,
        root: &Path,
    ) -> Result<Self> {
        manifest.verify_identity(binding)?;
        let root = root
            .canonicalize()
            .context("Optimization Definition snapshot is missing")?;
        let mut files = BTreeMap::new();
        for (reference, hash) in &manifest.files {
            let path = local_path(&root, reference)?;
            let bytes = fs::read(&path)?;
            if compute_sha256_hex(&bytes) != *hash {
                anyhow::bail!("Optimization Definition snapshot integrity mismatch: {reference}");
            }
            files.insert(
                reference.clone(),
                (bytes, fs::metadata(path)?.permissions()),
            );
        }
        Self::from_files(binding, manifest, &files)
    }
}

impl Manifest {
    fn verify_identity(&self, binding: &BoundOptimizationDefinition) -> Result<()> {
        if self.definition_sha256 != compute_sha256_hex(&serde_json::to_vec(&binding.definition)?) {
            anyhow::bail!("Optimization Definition snapshot identity mismatch");
        }
        let required = references(binding);
        if required != self.files.keys().cloned().collect() {
            anyhow::bail!(
                "Optimization Definition snapshot manifest does not match declared files"
            );
        }
        for evaluator in binding.requirements.requirements.evaluators.values() {
            if !self.files.contains_key(&evaluator.workflow) {
                anyhow::bail!(
                    "requirements reference evaluator outside the immutable snapshot: {}",
                    evaluator.workflow
                );
            }
        }
        Ok(())
    }

    pub fn verify(&self, binding: &BoundOptimizationDefinition, root: &Path) -> Result<()> {
        self.verify_identity(binding)?;
        let root = root
            .canonicalize()
            .context("Optimization Definition snapshot is missing")?;
        for (reference, hash) in &self.files {
            let path = local_path(&root, reference)?;
            if compute_sha256_hex(&fs::read(path)?) != *hash {
                anyhow::bail!("Optimization Definition snapshot integrity mismatch: {reference}");
            }
        }
        Ok(())
    }
}

pub(super) fn verify_journal(journal: &super::lifecycle::Journal, state_dir: &Path) -> Result<()> {
    runtime_from_journal(journal, state_dir).map(|_| ())
}

pub(super) fn runtime_from_journal(
    journal: &super::lifecycle::Journal,
    state_dir: &Path,
) -> Result<Runtime> {
    let manifest = journal.definition_snapshot.as_ref().context(
        "run has no immutable definition snapshot; start a new run, do not resume mutable source",
    )?;
    let expected = state_dir
        .join("optimize")
        .join(&journal.run_id)
        .join("definition")
        .canonicalize()?;
    if journal.definition_root != expected {
        anyhow::bail!("run definition_root must identify its persisted snapshot");
    }
    let binding = serde_json::from_value(journal.binding.clone())?;
    Runtime::load(&binding, manifest, &expected)
}
