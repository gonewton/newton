//! Same-process native execution with an independent, read-only Run observer.
//!
//! This embedding seam starts no HTTP listener. A host must authorize execution
//! and disclosure before calling it; it is not a remote work-ingress API.

use super::{native::NativeDriver, preflight, snapshot};
use anyhow::{Context, Result};
use newton_core::optimization::OptimizeRunObservation;
use newton_types::{optimization::*, BroadcastEvent};
use std::path::PathBuf;
use tokio::sync::broadcast;

/// Prepared native execution. The accompanying observer has no control methods.
/// Dropping an observer does not cancel this driver or release its ownership.
pub struct ObservedOptimizationRun {
    driver: NativeDriver,
}

impl ObservedOptimizationRun {
    /// Preflight, freeze declared inputs, and claim a new run using the normal
    /// native lifecycle. `binding` must come from the host's authorized binding
    /// process; no authority is inferred from the observer or an external tracker.
    ///
    /// `event_capacity` must be 1..=4096. Overflow triggers durable scoped snapshot
    /// recovery, not an unbounded event queue. The snapshot and future updates use
    /// the exact SQLite store and publisher owned by this native driver.
    pub async fn start(
        binding: BoundOptimizationDefinition,
        definition_root: PathBuf,
        state_dir: PathBuf,
        event_capacity: usize,
    ) -> Result<(Self, OptimizeRunObservation)> {
        anyhow::ensure!(
            (1..=4096).contains(&event_capacity),
            "observation event_capacity must be between 1 and 4096"
        );
        let (publisher, _) = broadcast::channel(event_capacity);
        Self::start_with_publisher(binding, definition_root, state_dir, publisher).await
    }

    async fn start_with_publisher(
        binding: BoundOptimizationDefinition,
        definition_root: PathBuf,
        state_dir: PathBuf,
        publisher: broadcast::Sender<BroadcastEvent>,
    ) -> Result<(Self, OptimizeRunObservation)> {
        uuid::Uuid::parse_str(&binding.run_id).context("Optimize Run requires a UUID")?;
        let run_id = binding.run_id.clone();
        let prepared = snapshot::Prepared::read(&binding, &definition_root)?;
        let runtime = prepared.runtime(&binding)?;
        preflight::check(&binding, &runtime).await?;
        prepared.verify_source(&binding, &definition_root)?;
        std::fs::create_dir_all(&state_dir)?;
        let state_dir = state_dir.canonicalize()?;
        let definition_root = prepared.persist(&state_dir, &run_id)?;
        let driver = NativeDriver::start(
            binding,
            definition_root,
            runtime,
            prepared.manifest,
            state_dir,
            publisher,
        )
        .await?;
        let observer = driver.observation_source().subscribe(&run_id).await?;
        Ok((Self { driver }, observer))
    }

    /// Execute the existing native loop. Observation failures are handled by the
    /// consumer separately and cannot manufacture completion or cancel this run.
    pub async fn run(self, once: bool, poll_interval_seconds: u64) -> Result<OptimizationOutcome> {
        self.driver.run(once, poll_interval_seconds).await
    }
}

#[cfg(test)]
mod tests;
