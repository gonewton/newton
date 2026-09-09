//! Fail-closed local ownership. An interrupted owner is never silently replaced.

use anyhow::{Context, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub(super) struct RunClaim {
    path: PathBuf,
    _lock: File,
}

impl RunClaim {
    /// Claim a context before dispatch. A surviving claim requires explicit
    /// reconciliation after a crash because external work may have completed.
    pub fn acquire(directory: &Path, run_id: &str) -> Result<Self> {
        Self::acquire_inner(directory, run_id, false)
    }

    pub fn resume(directory: &Path, run_id: &str) -> Result<Self> {
        Self::acquire_inner(directory, run_id, true)
    }

    fn acquire_inner(directory: &Path, run_id: &str, resume: bool) -> Result<Self> {
        fs::create_dir_all(directory)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("lock"))?;
        lock.try_lock()
            .context("optimization context has a live local owner")?;
        let path = directory.join("owner.json");
        if resume && path.exists() {
            let owner: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
            if owner.get("run_id").and_then(|v| v.as_str()) != Some(run_id) {
                anyhow::bail!("optimization context belongs to a different run");
            }
            fs::remove_file(&path)?;
        }
        let mut file = OpenOptions::new().write(true).create_new(true).open(&path)
            .with_context(|| format!("optimization context already claimed at {}; inspect its owner and reconcile unfinished work before retrying", path.display()))?;
        serde_json::to_writer(
            &mut file,
            &serde_json::json!({"run_id": run_id, "pid": std::process::id()}),
        )?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(Self { path, _lock: lock })
    }

    /// Release only after a durable terminal outcome with known external state.
    /// Drop deliberately does not release: cancellation/panic is not completion.
    pub fn release(self) -> Result<()> {
        fs::remove_file(&self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_consumer_cannot_claim_and_drop_does_not_replay() {
        let dir = tempfile::tempdir().unwrap();
        let claim = RunClaim::acquire(dir.path(), "first").unwrap();
        assert!(RunClaim::acquire(dir.path(), "second").is_err());
        drop(claim);
        assert!(RunClaim::acquire(dir.path(), "after-crash").is_err());
    }

    #[test]
    fn known_terminal_outcome_can_release_ownership() {
        let dir = tempfile::tempdir().unwrap();
        RunClaim::acquire(dir.path(), "first")
            .unwrap()
            .release()
            .unwrap();
        RunClaim::acquire(dir.path(), "second")
            .unwrap()
            .release()
            .unwrap();
    }
}
