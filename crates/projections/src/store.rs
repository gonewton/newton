use crate::{ProjectionError, ProjectionKey, ProjectionRecord};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

/// Storage-independent journal boundary with exclusive entity-scoped ownership.
pub trait ProjectionStore: Send + Sync {
    /// Acquire nonblocking exclusive ownership until the returned lease is dropped.
    /// Implementations MUST serialize delivery across processes, not only threads.
    fn acquire(&self, key: &ProjectionKey) -> Result<Box<dyn ProjectionLease>, ProjectionError>;
}

/// Exclusive delivery lease. Save success MUST mean durable atomic persistence.
pub trait ProjectionLease: Send + Sync {
    /// Load the current record; missing binding is returned as `None`.
    fn load(&self) -> Result<Option<ProjectionRecord>, ProjectionError>;
    /// Atomically persist a complete record before reporting success.
    fn save(&self, record: &ProjectionRecord) -> Result<(), ProjectionError>;
}

/// Local filesystem journal with atomic replacement and OS-released file locks.
///
/// The directory is caller-owned, usually beneath `.newton/projections`. Records
/// are keyed by SHA-256 of the internal identity; untrusted IDs never become paths.
/// Use a local filesystem with working advisory locks and atomic rename semantics.
#[derive(Debug, Clone)]
pub struct FileProjectionStore {
    directory: PathBuf,
}

impl FileProjectionStore {
    /// Select the journal directory without reading or changing domain state.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }
}

impl ProjectionStore for FileProjectionStore {
    fn acquire(&self, key: &ProjectionKey) -> Result<Box<dyn ProjectionLease>, ProjectionError> {
        if [&key.entity_kind, &key.entity_id, &key.destination]
            .iter()
            .any(|id| id.trim().is_empty())
        {
            return Err(ProjectionError::Invalid(
                "entity and destination identities are required".into(),
            ));
        }
        fs::create_dir_all(&self.directory)?;
        let name = hex::encode(Sha256::digest(serde_json::to_vec(key)?));
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.directory.join(format!("{name}.lock")))?;
        lock.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => ProjectionError::Busy,
            std::fs::TryLockError::Error(error) => ProjectionError::Io(error),
        })?;
        Ok(Box::new(FileLease {
            key: key.clone(),
            path: self.directory.join(format!("{name}.json")),
            _lock: lock,
        }))
    }
}

struct FileLease {
    key: ProjectionKey,
    path: PathBuf,
    _lock: File,
}

impl ProjectionLease for FileLease {
    fn load(&self) -> Result<Option<ProjectionRecord>, ProjectionError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let record: ProjectionRecord = serde_json::from_slice(&bytes)?;
        if record.key != self.key {
            return Err(ProjectionError::Invalid(
                "journal key does not match leased entity".into(),
            ));
        }
        Ok(Some(record))
    }

    fn save(&self, record: &ProjectionRecord) -> Result<(), ProjectionError> {
        if record.key != self.key {
            return Err(ProjectionError::Invalid(
                "cannot write another entity through this lease".into(),
            ));
        }
        let parent = self.path.parent().unwrap_or(Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer(&mut temporary, record)?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    }
}
