//! Filesystem helpers shared across Newton's state-writing code paths.
//!
//! Newton persists several kinds of state to disk as it runs — workflow
//! checkpoints, execution records, artifacts, and run completion envelopes —
//! and a half-written file for any of them (from a crash or a full disk
//! mid-write) is worse than a missing one: downstream tooling parses it as
//! garbage instead of noticing the run failed. [`atomic_write`] is the single
//! shared primitive all of those call sites use so that durability behavior
//! cannot drift between copies (spec 074, PR-3 / "B1 + S1").

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Atomically writes `bytes` to `path`.
///
/// The sequence is: ensure the parent directory exists, write `bytes` to a
/// temporary file in that SAME directory (so the final rename is a same-
/// filesystem, same-directory rename and therefore atomic), `fsync` the
/// temporary file so its contents are durable on disk, `rename` it over
/// `path`, then — on unix — best-effort `fsync` the parent directory so the
/// renamed directory entry itself survives a crash, not just the file's
/// bytes.
///
/// A reader of `path` therefore only ever observes either the previous
/// complete contents or the new complete contents; it can never observe a
/// truncated or partially written file.
///
/// The parent-directory fsync is `#[cfg(unix)]` and best-effort (its result
/// is not propagated) for two reasons: opening a directory with
/// [`File::open`] and calling [`File::sync_all`] on it is not a portable
/// operation — Windows does not support fsync-ing a directory handle this
/// way — and on unix the extra step only closes a narrow crash window around
/// the rename's directory-entry update, not the file-content durability
/// guarantee above, which already holds on every platform once this function
/// returns `Ok`.
///
/// # Errors
///
/// Returns an [`io::Error`] if the parent directory cannot be created, the
/// temporary file cannot be written or synced, or the rename fails (for
/// example because the containing directory is not writable, or the target
/// path is occupied by something the rename cannot replace).
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let parent: &Path = match parent {
        Some(p) => p,
        None => Path::new("."),
    };
    fs::create_dir_all(parent)?;

    let tmp_path = temp_path_for(path);

    let write_result = write_temp_file(&tmp_path, bytes);
    if let Err(err) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    fsync_parent_dir(parent);

    Ok(())
}

fn write_temp_file(tmp_path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(tmp_path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Deterministic temp-file name colocated with `path`, in the same
/// directory, so the subsequent rename never crosses a filesystem boundary.
fn temp_path_for(path: &Path) -> PathBuf {
    path.with_extension("tmp")
}

#[cfg(unix)]
fn fsync_parent_dir(parent: &Path) {
    // Best-effort: failures here (e.g. a filesystem that rejects opening
    // directories, or a permissions edge case) are deliberately swallowed.
    // The file-content durability guarantee documented on `atomic_write`
    // already holds without this step; it only narrows the crash window
    // around the rename's directory-entry update.
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
}

#[cfg(not(unix))]
fn fsync_parent_dir(_parent: &Path) {
    // Opening a directory as a `File` purely to fsync it is not a portable
    // operation: Windows' `CreateFileW` refuses directory handles unless
    // `FILE_FLAG_BACKUP_SEMANTICS` is set, which `std::fs::File::open` does
    // not set, so the call would simply fail every time. The rename above is
    // still durable per the target platform's own filesystem guarantees;
    // only the extra POSIX directory-entry durability margin is unavailable
    // here, and it is not worth a platform-specific syscall shim for a
    // best-effort step.
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_write_then_read() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("state.json");

        atomic_write(&target, b"hello world").expect("atomic_write should succeed");

        let read_back = fs::read(&target).expect("read back written file");
        assert_eq!(read_back, b"hello world");

        // No leftover temp file.
        assert!(!target.with_extension("tmp").exists());
    }

    #[test]
    fn overwrite_existing_file_replaces_contents() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("state.json");

        atomic_write(&target, b"first").expect("first write should succeed");
        atomic_write(&target, b"second-and-longer").expect("second write should succeed");

        let read_back = fs::read(&target).expect("read back written file");
        assert_eq!(read_back, b"second-and-longer");
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("nested").join("deeper").join("state.json");

        atomic_write(&target, b"payload").expect("should create parent dirs and write");

        assert_eq!(fs::read(&target).unwrap(), b"payload");
    }

    #[test]
    #[cfg(unix)]
    fn unwritable_directory_returns_err() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("state.json");

        let mut perms = fs::metadata(dir.path()).unwrap().permissions();
        perms.set_mode(0o500); // r-x: no write permission, so no new entries.
        fs::set_permissions(dir.path(), perms).unwrap();

        let result = atomic_write(&target, b"payload");

        // Restore write permission so the tempdir can clean itself up.
        let mut restore = fs::metadata(dir.path()).unwrap().permissions();
        restore.set_mode(0o700);
        fs::set_permissions(dir.path(), restore).unwrap();

        assert!(
            result.is_err(),
            "writing into a read-only directory must fail, not silently succeed"
        );
    }
}
