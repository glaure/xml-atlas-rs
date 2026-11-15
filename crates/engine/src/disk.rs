//! Telling when the file of an open document changed on disk (`docs/spec/open.md`, OPEN-14).

use std::fs::{self, Metadata};
use std::io;
use std::path::Path;
use std::time::SystemTime;

/// What a file looked like when the document last read or wrote it: its size and the time
/// it was last written. Another program writing the file changes at least one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
}

/// How the file differs from what the document last read or wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskChange {
    /// Something else wrote it.
    Changed,
    /// It is gone: deleted, moved or renamed.
    Removed,
}

impl FileStamp {
    pub fn from_metadata(meta: &Metadata) -> Self {
        Self {
            len: meta.len(),
            modified: meta.modified().ok(),
        }
    }

    /// The stamp of the file at `path` now; `None` when there is no file.
    pub fn of(path: &Path) -> io::Result<Option<Self>> {
        match fs::metadata(path) {
            Ok(meta) => Ok(Some(Self::from_metadata(&meta))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// How `now` differs from `known`, the stamp last read or written; `None` when it does not.
pub fn compare(known: Option<FileStamp>, now: Option<FileStamp>) -> Option<DiskChange> {
    match (known, now) {
        (a, b) if a == b => None,
        (_, None) => Some(DiskChange::Removed),
        (_, Some(_)) => Some(DiskChange::Changed),
    }
}
