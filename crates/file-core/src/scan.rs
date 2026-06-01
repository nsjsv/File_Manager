use std::ffi::OsString;
use std::io;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tokio::fs;

use crate::{sort_entries, DirectoryEntry, EntryMetadata, FileKind, SortDirection, SortField};

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

pub async fn scan_directory(
    path: impl AsRef<Path>,
    options: ScanOptions,
) -> Result<DirectoryScan, FileError> {
    let path = path.as_ref().to_path_buf();
    let mut reader = fs::read_dir(&path)
        .await
        .map_err(|source| FileError::ReadDirectory {
            path: path.clone(),
            source,
        })?;

    let mut entries = Vec::new();
    let mut skipped = Vec::new();

    loop {
        let dir_entry = match reader.next_entry().await {
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

        match entry_from_dir_entry(dir_entry, name, is_hidden).await {
            Ok(entry) => entries.push(entry),
            Err(FileError::Metadata { path, source }) => skipped.push(ScanWarning {
                path,
                message: source.to_string(),
            }),
            Err(error) => return Err(error),
        }
    }

    sort_entries(&mut entries, &options);

    Ok(DirectoryScan {
        path,
        entries,
        skipped,
    })
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
    #[error("could not search {path:?}: {message}")]
    SearchIndex { path: PathBuf, message: String },
    #[error("invalid input for {path:?}: {message}")]
    InvalidInput { path: PathBuf, message: String },
    #[error("operation cancelled")]
    Cancelled,
    #[error("unsupported operation: {0}")]
    Unsupported(&'static str),
}
