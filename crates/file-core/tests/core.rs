use std::ffi::OsStr;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::{fs, io};

use file_core::{
    available_transfer_target_path, check_transfer_conflicts, copy_path, copy_path_with_options,
    create_archive_with_progress, create_directory, create_empty_file, create_file_with_contents,
    delete_path_permanently, extract_archive, filter_hidden, is_transfer_target_available,
    move_path, move_path_with_options, rename_path, scan_directory, scan_directory_with_progress,
    sort_entries, watch_directory, ArchiveCompressionLevel, ArchiveCreationRequest,
    ArchiveExtractionRequest, ArchiveFormat, DirectoryEntry, EntryMetadata, FileError, FileKind,
    FileOperationControls, FileOperationRunState, FileOperationVerification, FileTransferOptions,
    ScanOptions, SortDirection, SortField, TransferConflictCheck, TransferConflictStrategy,
};
use tempfile::tempdir;

fn entry(path: PathBuf, kind: FileKind, len: u64, is_hidden: bool) -> DirectoryEntry {
    DirectoryEntry::new(
        path,
        kind,
        EntryMetadata {
            len,
            modified: None,
            readonly: false,
        },
        is_hidden,
        false,
        false,
    )
}

fn names(entries: &[DirectoryEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| entry.name().to_string_lossy().into_owned())
        .collect()
}

#[path = "core/archive.rs"]
mod archive;
#[path = "core/ops.rs"]
mod ops;
#[path = "core/scan.rs"]
mod scan;
#[path = "core/sort.rs"]
mod sort;
#[path = "core/watch.rs"]
mod watch;
