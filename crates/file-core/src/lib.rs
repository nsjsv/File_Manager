pub mod entry;
pub mod ops;
pub mod scan;
pub mod search;
pub mod sort;
pub mod trash_bin;
pub mod watch;

pub use entry::{DirectoryEntry, EntryMetadata, FileKind};
pub use ops::{
    copy_path, copy_path_with_conflict_strategy, copy_path_with_controls,
    copy_path_with_controls_and_strategy, create_directory, create_empty_file,
    create_file_with_contents, move_path, move_path_with_conflict_strategy,
    move_path_with_controls, move_path_with_controls_and_strategy, rename_path, trash_path,
    CopyProgress, FileOperationControls, FileOperationRunState, ProgressSender,
    TransferConflictStrategy,
};
pub use scan::{scan_directory, DirectoryScan, FileError, ScanOptions, ScanWarning};
pub use search::{
    build_file_search_index, file_search_index_exists, search_file_index, search_file_tree,
    FileSearchIndexOptions, FileSearchIndexOutcome, FileSearchMatch, FileSearchOptions,
    FileSearchOutcome,
};
pub use sort::{
    apply_entry_options, compare_entries, filter_hidden, sort_entries, SortDirection, SortField,
};
pub use trash_bin::{
    delete_trash_entry, empty_trash, restore_trash_entry, scan_trash, TrashEntry,
    TrashRestoreEntry, TrashScan,
};
pub use watch::{watch_directory, DirectoryChange, DirectoryWatcher};
