//! Shared test-support helpers for `newton` CLI E2E tests (spec 301).
//!
//! Files include this module via `#[path = "../support/mod.rs"] mod support;`.
//!
//! ## Why `#![allow(dead_code)]`
//! This module is compiled once per test binary that includes it via `#[path]`.
//! Each binary uses a different subset of helpers, so items that are live in one
//! binary appear dead in another. The module-level suppression avoids per-item
//! annotations for every cross-binary false-positive. Extended-tier helpers
//! (`spawn_with_timeout`, `ExitOutcome`, `newton_std`) are currently unused by
//! any active test binary — they will be activated in Stages 2–4.
#![allow(dead_code)]

use assert_cmd::Command;
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tempfile::TempDir;
use wait_timeout::ChildExt;

pub const KILL_WAIT_TIMEOUT_SECS: u64 = 5;

pub fn fixture_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

/// Build a `newton` Command with a clean env.
pub fn newton() -> Command {
    let mut cmd = Command::cargo_bin("newton").expect("newton binary builds");
    cmd.env("NEWTON_LOG", "warn");
    cmd
}

/// Same binary as a `std::process::Command`, for code paths that need
/// process-level control (kill, signal handling).
pub fn newton_std() -> std::process::Command {
    let path = assert_cmd::cargo::cargo_bin("newton");
    let mut c = std::process::Command::new(path);
    c.env("NEWTON_LOG", "warn");
    c
}

#[derive(Copy, Clone, Debug)]
pub enum RunStatus {
    Completed,
    Failed,
    Running,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        // Matches `WorkflowExecutionStatus::as_str` in the production code.
        match self {
            RunStatus::Completed => "Completed",
            RunStatus::Failed => "Failed",
            RunStatus::Running => "Running",
        }
    }
}

pub struct TempWorkspace {
    pub dir: TempDir,
}

impl Default for TempWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

impl TempWorkspace {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        fs::create_dir_all(p.join(".newton/state/workflows")).unwrap();
        fs::create_dir_all(p.join(".newton/artifacts")).unwrap();
        fs::create_dir_all(p.join(".newton/scripts")).unwrap();
        Self { dir }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Seed `.newton/state/workflows/<run_id>/` with minimal `execution.json`
    /// and `checkpoint.json` matching the production schema, so `runs list`,
    /// `runs show`, `resume`, and `checkpoint list` see at least one run.
    ///
    /// `run_id` MUST be a valid UUID string — this matches what the runtime
    /// stores. `runs list` only emits rows when the directory name equals
    /// `execution_id` in the JSON.
    pub fn seed_run(&self, run_id: &str, status: RunStatus) -> PathBuf {
        let run_dir = self.path().join(".newton/state/workflows").join(run_id);
        fs::create_dir_all(&run_dir).unwrap();
        let now = Utc::now();
        let started_at = now.to_rfc3339();
        let completed_at = (now + chrono::Duration::seconds(1)).to_rfc3339();
        let execution = serde_json::json!({
            "format_version": "1",
            "execution_id": run_id,
            "workflow_file": "minimal_smoke.yaml",
            "workflow_version": "2.0",
            "workflow_hash": "0000000000000000000000000000000000000000000000000000000000000000",
            "started_at": started_at,
            "completed_at": completed_at,
            "status": status.as_str(),
            "task_runs": [],
            "settings_effective": {},
            "nesting_depth": 0,
        });
        fs::write(
            run_dir.join("execution.json"),
            serde_json::to_string_pretty(&execution).unwrap(),
        )
        .unwrap();
        let checkpoint = serde_json::json!({
            "format_version": "1",
            "execution_id": run_id,
            "workflow_hash": "0000000000000000000000000000000000000000000000000000000000000000",
            "created_at": completed_at,
            "ready_queue": [],
            "context": {},
            "trigger_payload": {},
            "task_iterations": {},
        });
        fs::write(
            run_dir.join("checkpoint.json"),
            serde_json::to_string_pretty(&checkpoint).unwrap(),
        )
        .unwrap();
        run_dir
    }

    pub fn write_workflow(&self, name: &str, yaml: &str) -> PathBuf {
        let p = self.path().join(name);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, yaml).unwrap();
        p
    }

    /// Create a non-empty file under `.newton/artifacts/<sub>/` and
    /// return its path. Used by artifact-clean integration tests.
    pub fn write_artifact(&self, sub: &str, name: &str, contents: &[u8]) -> PathBuf {
        let dir = self.path().join(".newton/artifacts").join(sub);
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::write(&p, contents).unwrap();
        p
    }
}

pub struct ExitOutcome {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Spawn a command and wait up to `timeout`. On timeout the child is sent
/// SIGTERM (best effort), then SIGKILL after `KILL_WAIT_TIMEOUT_SECS`.
///
/// Uses `wait_timeout::ChildExt::wait_timeout` to block without spinning.
pub fn spawn_with_timeout(mut std_cmd: std::process::Command, timeout: Duration) -> ExitOutcome {
    std_cmd.stdout(Stdio::piped());
    std_cmd.stderr(Stdio::piped());
    let mut child = std_cmd.spawn().expect("spawn child");

    let (status, timed_out) = match child.wait_timeout(timeout).expect("wait_timeout") {
        Some(s) => (Some(s), false),
        None => {
            // Timed out — ask nicely first, then force.
            send_sigterm(child.id());
            let graceful = child
                .wait_timeout(Duration::from_secs(KILL_WAIT_TIMEOUT_SECS))
                .expect("wait_timeout after sigterm");
            if graceful.is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
            (graceful, true)
        }
    };

    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut out) = child.stdout.take() {
        use std::io::Read;
        let _ = out.read_to_string(&mut stdout);
    }
    if let Some(mut err) = child.stderr.take() {
        use std::io::Read;
        let _ = err.read_to_string(&mut stderr);
    }
    ExitOutcome {
        status: status.and_then(|s| s.code()),
        stdout,
        stderr,
        timed_out,
    }
}

#[cfg(unix)]
fn send_sigterm(pid: u32) {
    let _ = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
}

#[cfg(not(unix))]
fn send_sigterm(_pid: u32) {
    // No portable graceful-stop on non-unix; caller falls through to kill().
}
