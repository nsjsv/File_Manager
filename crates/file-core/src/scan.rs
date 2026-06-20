use std::ffi::OsString;
use std::io;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tokio::fs;
use tokio_util::sync::CancellationToken;

use crate::{sort_entries, DirectoryEntry, EntryMetadata, FileKind, SortDirection, SortField};

const DIRECTORY_SCAN_BATCH_SIZE: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOptions {
    pub include_hidden: bool,
    pub sort_field: SortField,
    pub sort_direction: SortDirection,
    pub directories_first: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            include_hidden: false,
            sort_field: SortField::Name,
            sort_direction: SortDirection::Ascending,
            directories_first: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryScan {
    pub path: PathBuf,
    pub entries: Vec<DirectoryEntry>,
    pub skipped: Vec<ScanWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanWarning {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryScanBatch {
    pub path: PathBuf,
    pub entries: Vec<DirectoryEntry>,
    pub skipped: Vec<ScanWarning>,
}

pub async fn scan_directory(
    path: impl AsRef<Path>,
    options: ScanOptions,
) -> Result<DirectoryScan, FileError> {
    scan_directory_with_progress(path, options, CancellationToken::new(), |_| {}).await
}

pub async fn scan_directory_with_progress(
    path: impl AsRef<Path>,
    options: ScanOptions,
    cancellation: CancellationToken,
    mut on_batch: impl FnMut(DirectoryScanBatch),
) -> Result<DirectoryScan, FileError> {
    let path = path.as_ref().to_path_buf();
    if cancellation.is_cancelled() {
        return Err(FileError::Cancelled);
    }
    let mut reader = fs::read_dir(&path)
        .await
        .map_err(|source| FileError::ReadDirectory {
            path: path.clone(),
            source,
        })?;

    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    let mut batch_entries = Vec::new();
    let mut batch_skipped = Vec::new();

    loop {
        let next_entry = tokio::select! {
            _ = cancellation.cancelled() => return Err(FileError::Cancelled),
            next_entry = reader.next_entry() => next_entry,
        };
        let dir_entry = match next_entry {
            Ok(Some(dir_entry)) => dir_entry,
            Ok(None) => break,
            Err(source) => {
                return Err(FileError::ReadEntry {
                    path: path.clone(),
                    source,
                })
            }
        };

        let name = dir_entry.file_name();
        let is_hidden = is_hidden_name(&name);
        if is_hidden && !options.include_hidden {
            continue;
        }

        let entry_result = tokio::select! {
            _ = cancellation.cancelled() => return Err(FileError::Cancelled),
            entry_result = entry_from_dir_entry(dir_entry, name, is_hidden) => entry_result,
        };
        match entry_result {
            Ok(entry) => {
                entries.push(entry.clone());
                batch_entries.push(entry);
            }
            Err(FileError::Metadata { path, source }) => {
                let warning = ScanWarning {
                    path,
                    message: source.to_string(),
                };
                skipped.push(warning.clone());
                batch_skipped.push(warning);
            }
            Err(error) => return Err(error),
        }

        if batch_entries.len() + batch_skipped.len() >= DIRECTORY_SCAN_BATCH_SIZE {
            emit_directory_scan_batch(&path, &mut batch_entries, &mut batch_skipped, &mut on_batch);
        }
    }

    emit_directory_scan_batch(&path, &mut batch_entries, &mut batch_skipped, &mut on_batch);

    sort_entries(&mut entries, &options);

    Ok(DirectoryScan {
        path,
        entries,
        skipped,
    })
}

fn emit_directory_scan_batch(
    path: &Path,
    entries: &mut Vec<DirectoryEntry>,
    skipped: &mut Vec<ScanWarning>,
    on_batch: &mut impl FnMut(DirectoryScanBatch),
) {
    if entries.is_empty() && skipped.is_empty() {
        return;
    }

    on_batch(DirectoryScanBatch {
        path: path.to_path_buf(),
        entries: std::mem::take(entries),
        skipped: std::mem::take(skipped),
    });
}

pub(crate) async fn entry_from_dir_entry(
    dir_entry: fs::DirEntry,
    name: std::ffi::OsString,
    is_hidden: bool,
) -> Result<DirectoryEntry, FileError> {
    let path = dir_entry.path();
    entry_from_path(path, name, is_hidden).await
}

pub(crate) async fn entry_from_path(
    path: PathBuf,
    name: OsString,
    is_hidden: bool,
) -> Result<DirectoryEntry, FileError> {
    let symlink_metadata =
        fs::symlink_metadata(&path)
            .await
            .map_err(|source| FileError::Metadata {
                path: path.clone(),
                source,
            })?;
    let file_type = symlink_metadata.file_type();
    let is_symlink = file_type.is_symlink();
    let is_broken_symlink = if is_symlink {
        matches!(fs::metadata(&path).await, Err(error) if error.kind() == io::ErrorKind::NotFound)
    } else {
        false
    };

    let kind = if file_type.is_dir() {
        FileKind::Directory
    } else if file_type.is_file() {
        FileKind::File
    } else if is_symlink {
        FileKind::Symlink
    } else {
        FileKind::Other
    };

    let metadata = EntryMetadata {
        len: symlink_metadata.len(),
        modified: symlink_metadata.modified().ok(),
        readonly: symlink_metadata.permissions().readonly(),
    };

    Ok(DirectoryEntry::with_file_name(
        path,
        name,
        kind,
        metadata,
        is_hidden,
        is_symlink,
        is_broken_symlink,
    ))
}

#[cfg(unix)]
pub(crate) fn is_hidden_name(name: &std::ffi::OsStr) -> bool {
    name.as_bytes().first() == Some(&b'.')
}

#[cfg(not(unix))]
pub(crate) fn is_hidden_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

#[derive(Debug, Error)]
pub enum FileError {
    #[error("could not read directory {path:?}: {source}")]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read directory entry in {path:?}: {source}")]
    ReadEntry {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read metadata for {path:?}: {source}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not create directory {path:?}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not create file {path:?}: {source}")]
    CreateFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not rename {from:?} to {to:?}: {source}")]
    Rename {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not copy {from:?} to {to:?}: {source}")]
    Copy {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not move {from:?} to {to:?}: {source}")]
    Move {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not move {path:?} to trash: {message}")]
    Trash { path: PathBuf, message: String },
    #[error("could not watch {path:?}: {message}")]
    Watch { path: PathBuf, message: String },
    #[error("could not create archive {path:?}: {message}")]
    Archive { path: PathBuf, message: String },
    #[error("archive {path:?} requires a password")]
    ArchivePasswordRequired { path: PathBuf },
    #[error("archive {path:?} password is incorrect")]
    ArchiveInvalidPassword { path: PathBuf },
    #[error("invalid input for {path:?}: {message}")]
    InvalidInput { path: PathBuf, message: String },
    #[error("operation cancelled")]
    Cancelled,
    #[error("unsupported operation: {0}")]
    Unsupported(&'static str),
}
