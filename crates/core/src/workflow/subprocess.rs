//! Shared subprocess-spawning helpers for workflow operators.
//!
//! Every operator that shells out (`GitOperator`, `GhOperator`,
//! `CommandOperator`'s [`TokioCommandRunner`](super::operators::command),
//! and `AgentOperator`'s engine child) needs the same belt-and-braces
//! cleanup: `kill_on_drop(true)` alone only reaps the *direct* child if the
//! owning future is dropped (e.g. an outer per-task `timeout_ms` firing) —
//! any grandchildren the child spawns (e.g. a backgrounded `sleep &`) are
//! orphaned. [`ProcessGroupKillGuard`] closes that gap on unix by putting
//! the child in its own process group and `killpg`-ing the whole group from
//! `Drop` if the guard is still armed. [`run_guarded`] wires that guard
//! around a plain "spawn, wait, collect output" flow for operators that
//! don't need to stream output while the child runs; `AgentOperator` keeps
//! its own bespoke streaming flow but reuses [`ProcessGroupKillGuard`]
//! directly (see `workflow::operators::agent::command`).

use std::process::Output;
use tokio::process::Command;

/// Configure `cmd` for group-wide cleanup: `kill_on_drop(true)` always,
/// plus (unix only) making the child the leader of its own new process
/// group so grandchildren it spawns share that group and can be killed as a
/// unit via [`ProcessGroupKillGuard`]. Non-unix: `kill_on_drop` alone; there
/// is no portable equivalent of `process_group`, so grandchildren can leak
/// there (documented gap, matches [`ProcessGroupKillGuard`]'s non-unix
/// no-op).
pub(crate) fn prepare_command_for_group_kill(cmd: &mut Command) {
    cmd.kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
}

/// Group-wide kill guard for a spawned child, owned by the future that
/// spawned it (i.e. a plain local, never detached via `tokio::spawn`).
///
/// The child must be spawned with `process_group(0)` (unix) — see
/// [`prepare_command_for_group_kill`] — making it the leader of its own new
/// process group. Grandchildren the child spawns (e.g. a background `sleep
/// &`) inherit that group. `kill_on_drop` alone only reaps the *direct*
/// child on drop — grandchildren are orphaned. This guard closes that gap:
/// on `Drop`, while still armed, it sends `SIGKILL` to the whole process
/// group via `killpg`, so it fires whenever the owning future is dropped
/// before a clean wait — whether that's an operator-internal timeout
/// returning early, or an *outer* per-task timeout dropping the whole
/// future (including this guard's stack frame) without ever calling
/// `Child::kill`.
///
/// Both paths converge on the same `Drop` impl, so the mechanism is
/// cancellation-safe by construction: it does not depend on any code path
/// explicitly running to completion.
///
/// Call [`ProcessGroupKillGuard::disarm`] only after `Child::wait()` (or an
/// equivalent that consumes the child, e.g. `wait_with_output()`) has
/// returned successfully (a "clean wait") — at that point the direct child
/// is confirmed reaped and killing the group is no longer this guard's job.
/// Disarm as soon as possible after that point and before any further
/// await: if the guard were still armed across a later await point, an
/// outer future-drop there could `killpg()` a pgid whose leader was already
/// reaped, which — if the rest of the group had also already exited — the
/// OS could have recycled for an unrelated process group. On a *failed*
/// wait the child's process/group state is unknown, so the guard should be
/// left armed as the safety net.
///
/// Non-unix: `process_group`/`killpg` have no portable equivalent, so this
/// is a documented no-op there; `kill_on_drop(true)` (set unconditionally
/// by [`prepare_command_for_group_kill`]) still reaps the direct child, but
/// grandchildren can leak on non-unix platforms.
pub(crate) struct ProcessGroupKillGuard {
    #[cfg(unix)]
    pgid: libc::pid_t,
    #[cfg(unix)]
    armed: bool,
}

impl ProcessGroupKillGuard {
    /// `pid` is the freshly spawned child's pid. Because the child is
    /// spawned with `process_group(0)`, it is its own process group leader,
    /// so `pgid == pid`.
    #[cfg(unix)]
    pub(crate) fn new(pid: u32) -> Self {
        Self {
            pgid: pid as libc::pid_t,
            armed: true,
        }
    }

    #[cfg(not(unix))]
    pub(crate) fn new(_pid: u32) -> Self {
        Self {}
    }

    /// Disarm after a clean wait/exit so `Drop` becomes a no-op. Must not
    /// be called before the direct child is confirmed reaped.
    pub(crate) fn disarm(&mut self) {
        #[cfg(unix)]
        {
            self.armed = false;
        }
    }
}

#[cfg(unix)]
impl Drop for ProcessGroupKillGuard {
    fn drop(&mut self) {
        if self.armed {
            // SAFETY: plain FFI call with a pgid/signal pair, no pointers
            // involved. If the group is already gone (process exited and
            // was reaped, e.g. via the belt-and-braces `kill_on_drop`)
            // `killpg` just returns ESRCH; cleanup here is intentionally
            // best-effort and errors are not actionable in a `Drop` impl.
            unsafe {
                libc::killpg(self.pgid, libc::SIGKILL);
            }
        }
    }
}

/// Spawn `cmd` (already built with args/cwd/env/stdio set by the caller)
/// with group-wide kill protection, wait for it to complete, and return the
/// captured [`Output`] — a cancellation-safe, process-group-aware analogue
/// of `tokio::process::Command::output()` (equivalently,
/// `child.wait_with_output()`).
///
/// `kill_on_drop(true)` and (unix) `process_group(0)` are applied here via
/// [`prepare_command_for_group_kill`]; callers must not set those
/// themselves. Stdio capture semantics match `Command::output()`: a
/// stream not explicitly `Stdio::piped()` is simply not captured (empty
/// `Vec` in the returned `Output`).
///
/// If the returned future is dropped before completion (e.g. an outer
/// per-task timeout), `kill_on_drop` reaps the direct child and, on unix,
/// the still-armed [`ProcessGroupKillGuard`] SIGKILLs the whole process
/// group on its own drop, cleaning up any grandchildren too. See that
/// type's docs for the disarm-ordering rationale this function follows:
/// the guard is armed the instant `spawn()` succeeds (before any await
/// point) and disarmed immediately after `wait_with_output` returns `Ok` —
/// there is no further await after that point in this function, so the
/// disarm is the last thing that happens before returning.
pub(crate) async fn run_guarded(mut cmd: Command) -> std::io::Result<Output> {
    prepare_command_for_group_kill(&mut cmd);
    let child = cmd.spawn()?;

    // Armed immediately after spawn, before any await point that could be
    // cancelled by an outer timeout. See `ProcessGroupKillGuard` docs for
    // why this must happen here rather than deferred.
    let mut guard =
        ProcessGroupKillGuard::new(child.id().expect("freshly spawned child must have a pid"));

    let result = child.wait_with_output().await;

    // Disarm immediately after a clean wait, before returning — see
    // `ProcessGroupKillGuard::disarm` docs. On error the child's
    // process/group state is unknown, so the guard is deliberately left
    // armed as the safety net.
    if result.is_ok() {
        guard.disarm();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    // ── shared guarded-run cleanup ───────────────────────────────────────
    //
    // Mirrors the polling patterns used by the agent operator's PR-7 tests
    // (`workflow::operators::agent::command::tests`): a fixture shell
    // script backgrounds a `sleep 300` grandchild (writing its pid to a
    // file so the test can find it), then loops forever appending a byte to
    // a "heartbeat" file. The script never exits on its own — the test
    // relies entirely on dropping the `run_guarded` future (via an outer
    // `tokio::time::timeout`) to end it, proving cleanup is cancellation
    // driven rather than dependent on any explicit kill call running to
    // completion.

    #[cfg(unix)]
    fn grandchild_leak_script(heartbeat: &Path, grandchild_pid_file: &Path) -> String {
        format!(
            r#"( sleep 300 & echo $! > "{grandchild_pid}" )
while true; do printf x >> "{heartbeat}"; sleep 0.02; done"#,
            grandchild_pid = grandchild_pid_file.display(),
            heartbeat = heartbeat.display(),
        )
    }

    #[cfg(unix)]
    async fn wait_for_heartbeat_to_stop(path: &Path, quiet: Duration, max_wait: Duration) -> bool {
        let poll = Duration::from_millis(20);
        let deadline = Instant::now() + max_wait;
        let mut last_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let mut quiet_since = Instant::now();
        loop {
            tokio::time::sleep(poll).await;
            let size = std::fs::metadata(path)
                .map(|m| m.len())
                .unwrap_or(last_size);
            if size != last_size {
                last_size = size;
                quiet_since = Instant::now();
            } else if quiet_since.elapsed() >= quiet {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
        }
    }

    #[cfg(unix)]
    async fn wait_for_file_nonempty(path: &Path, max_wait: Duration) -> bool {
        let poll = Duration::from_millis(20);
        let deadline = Instant::now() + max_wait;
        loop {
            if std::fs::metadata(path)
                .map(|m| m.len() > 0)
                .unwrap_or(false)
            {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(poll).await;
        }
    }

    #[cfg(unix)]
    fn read_pid_file(path: &Path) -> libc::pid_t {
        std::fs::read_to_string(path)
            .expect("read pid file")
            .trim()
            .parse()
            .expect("pid file contains a valid pid")
    }

    #[cfg(unix)]
    async fn wait_for_pid_death(pid: libc::pid_t, max_wait: Duration) -> bool {
        let poll = Duration::from_millis(20);
        let deadline = Instant::now() + max_wait;
        loop {
            // SAFETY: signal 0 touches no memory; it only probes whether
            // the pid exists and is signalable by us.
            let alive = unsafe { libc::kill(pid, 0) == 0 };
            if !alive {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(poll).await;
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_guarded_kills_process_group_on_outer_future_drop() {
        let tmp = TempDir::new().unwrap();
        let heartbeat = tmp.path().join("heartbeat");
        let grandchild_pid_file = tmp.path().join("grandchild.pid");
        let script = grandchild_leak_script(&heartbeat, &grandchild_pid_file);

        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(script)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null());

        // The fixture script never exits, so this timeout always elapses,
        // dropping the `run_guarded` future without it ever reaching a
        // clean `wait_with_output` return.
        let outcome = tokio::time::timeout(Duration::from_millis(200), run_guarded(cmd)).await;
        assert!(
            outcome.is_err(),
            "expected the outer timeout to fire before the fixture script exits on its own"
        );

        assert!(
            wait_for_file_nonempty(&grandchild_pid_file, Duration::from_secs(2)).await,
            "grandchild pid file was never written"
        );
        let grandchild_pid = read_pid_file(&grandchild_pid_file);

        assert!(
            wait_for_heartbeat_to_stop(
                &heartbeat,
                Duration::from_millis(200),
                Duration::from_secs(3)
            )
            .await,
            "direct child kept writing to heartbeat after outer future drop; not killed"
        );
        assert!(
            wait_for_pid_death(grandchild_pid, Duration::from_secs(3)).await,
            "grandchild process survived the process-group kill (run_guarded future-drop path)"
        );
    }

    #[tokio::test]
    async fn run_guarded_returns_output_on_clean_exit() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("echo hello")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null());

        let output = run_guarded(cmd).await.expect("command must run");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
    }
}
