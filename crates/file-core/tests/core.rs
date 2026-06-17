use std::ffi::OsStr;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::{fs, io};

use file_core::{
    available_transfer_target_path, build_file_search_index, check_transfer_conflicts, copy_path,
    copy_path_with_options, create_archive_with_progress, create_directory, create_empty_file,
    create_file_with_contents, extract_archive, file_search_index_exists, filter_hidden,
    is_transfer_target_available, move_path, move_path_with_options, rename_path, scan_directory,
    search_file_index, search_file_tree, sort_entries, watch_directory, ArchiveCompressionLevel,
    ArchiveCreationRequest, ArchiveExtractionRequest, ArchiveFormat, DirectoryEntry, EntryMetadata,
    FileError, FileKind, FileOperationControls, FileOperationRunState, FileOperationVerification,
    FileSearchIndexOptions, FileSearchOptions, FileTransferOptions, ScanOptions, SortDirection,
    SortField, TransferConflictCheck, TransferConflictStrategy,
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
#[path = "core/search.rs"]
mod search;
#[path = "core/sort.rs"]
mod sort;
#[path = "core/watch.rs"]
mod watch;
