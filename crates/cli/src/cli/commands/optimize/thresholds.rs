//! Per-objective, revision-scoped stopping guards; there is no aggregate Grade.

use anyhow::{Context, Result};
use newton_types::optimization::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct History {
    pub revisions: BTreeMap<u64, BTreeMap<String, Progress>>,
    pub stop: Option<OptimizationStopReason>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Progress {
    pub baseline: f64,
    pub best: f64,
    pub no_progress: u64,
    pub last_cycle: Option<u64>,
    pub observations: Vec<(u64, f64)>,
    #[serde(default)]
    pub fewest_open_findings: Option<u64>,
}

impl History {
    /// Evidence has already passed the common identity, units and sample checks.
    pub fn observe(
        &mut self,
        requirements: &RequirementsRevision,
        evidence: &CandidateEvaluation,
        baseline: bool,
        open_findings: Option<&BTreeMap<String, u64>>,
    ) -> Result<Option<OptimizationStopReason>> {
        let ObjectiveMode::Thresholds { objectives } = &requirements.requirements.objective else {
            return Ok(None);
        };
        let progress = self.revisions.entry(requirements.revision).or_default();
        self.stop = None;
        self.diagnostics.clear();
        let mut stalled = false;
        let mut regressed = false;
        for threshold in objectives {
            let id = &threshold.objective.id;
            let Some(ObjectiveMeasurement::Produced { samples, .. }) =
                evidence.measurements.get(id)
            else {
                anyhow::bail!("threshold guard requires complete evidence for {id}");
            };
            let score = samples.iter().copied().sum::<f64>() / samples.len() as f64;
            if !score.is_finite() {
                anyhow::bail!("threshold guard requires finite samples for {id}");
            }
            let count = open_findings.and_then(|counts| counts.get(id)).copied();
            let current = progress.entry(id.clone()).or_insert_with(|| Progress {
                baseline: score,
                best: score,
                no_progress: 0,
                last_cycle: None,
                observations: Vec::new(),
                fewest_open_findings: count,
            });
            if current.baseline - score > threshold.regression_delta {
                regressed = true;
                self.diagnostics.push(format!(
                    "{id} regressed from baseline {} to {score}, beyond tolerance {}",
                    current.baseline, threshold.regression_delta
                ));
            }
            if !baseline && current.last_cycle != Some(evidence.cycle) {
                let finding_progress = matches!((count, current.fewest_open_findings), (Some(count), Some(previous)) if count < previous);
                current.no_progress =
                    if score > current.best || score >= threshold.target || finding_progress {
                        0
                    } else {
                        current
                            .no_progress
                            .checked_add(1)
                            .context("no-progress counter overflow")?
                    };
                current.best = current.best.max(score);
                if let Some(count) = count {
                    current.fewest_open_findings = Some(
                        current
                            .fewest_open_findings
                            .map_or(count, |prior| prior.min(count)),
                    );
                }
                current.last_cycle = Some(evidence.cycle);
                current.observations.push((evidence.cycle, score));
            }
            if baseline {
                current.baseline = score;
            }
            if current.no_progress >= threshold.no_progress_cycles {
                stalled = true;
                self.diagnostics.push(format!(
                    "{id} made no progress for {} cycles below target {}",
                    current.no_progress, threshold.target
                ));
            }
        }
        self.stop = if regressed {
            Some(OptimizationStopReason::Regression)
        } else if stalled {
            Some(OptimizationStopReason::NoProgress)
        } else {
            None
        };
        Ok(self.stop)
    }
}
