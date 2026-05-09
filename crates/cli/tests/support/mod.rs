//! Shared test-support helpers for `newton` CLI E2E tests (spec 301).
//!
//! Files include this module via `#[path = "../support/mod.rs"] mod support;`.

#![allow(dead_code)]

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;

pub const KILL_WAIT_TIMEOUT_SECS: u64 = 5;

/// Build a `newton` Command with a clean env.
pub fn newton() -> Command {
    let mut cmd = Command::cargo_bin("newton").expect("newton binary builds");
    cmd.env("NEWTON_LOG", "warn");
    cmd
}

#[derive(Copy, Clone, Debug)]
pub enum RunStatus {
    Completed,
    Failed,
    Running,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Completed => "completed",
            RunStatus::Failed => "failed",
            RunStatus::Running => "running",
        }
    }
}

pub struct TempWorkspace {
    pub dir: TempDir,
}

impl TempWorkspace {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        fs::create_dir_all(p.join(".newton/state/workflows")).unwrap();
        fs::create_dir_all(p.join(".newton/state/artifacts")).unwrap();
        fs::create_dir_all(p.join(".newton/scripts")).unwrap();
        Self { dir }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Seed `.newton/state/workflows/<run_id>/` with minimal `execution.json`
    /// and `checkpoint.json` so `runs list|show`, `resume`, `checkpoint list`
    /// have something to read.
    pub fn seed_run(&self, run_id: &str, status: RunStatus) -> PathBuf {
        let run_dir = self.path().join(".newton/state/workflows").join(run_id);
        fs::create_dir_all(&run_dir).unwrap();
        let execution = serde_json::json!({
            "run_id": run_id,
            "status": status.as_str(),
            "started_at": "2026-01-01T00:00:00Z",
            "workflow_path": "minimal_smoke.yaml",
            "tasks": [],
        });
        fs::write(
            run_dir.join("execution.json"),
            serde_json::to_string_pretty(&execution).unwrap(),
        )
        .unwrap();
        let checkpoint = serde_json::json!({
            "run_id": run_id,
            "iteration": 0,
            "completed_tasks": [],
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
}

pub struct ExitOutcome {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Spawn a command and wait up to `timeout`. On timeout the child is killed.
pub fn spawn_with_timeout(mut std_cmd: std::process::Command, timeout: Duration) -> ExitOutcome {
    std_cmd.stdout(std::process::Stdio::piped());
    std_cmd.stderr(std::process::Stdio::piped());
    let mut child = std_cmd.spawn().expect("spawn child");
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break None,
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
