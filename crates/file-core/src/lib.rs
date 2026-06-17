pub mod archive;
pub mod archive_extraction;
pub mod entry;
pub mod media;
pub mod ops;
pub mod scan;
pub mod search;
pub mod sort;
pub mod transfer_conflict;
pub mod trash_bin;
pub mod watch;

pub use archive::{
    create_archive_with_progress, ArchiveCompressionLevel, ArchiveCreationProgress,
    ArchiveCreationRequest, ArchiveFormat, ArchivePassword,
};
pub use archive_extraction::{
    archive_extraction_format_for_path, extract_archive, inspect_archive_extraction,
    is_supported_archive_path, ArchiveExtractionFormat, ArchiveExtractionRequest,
};
pub use entry::{DirectoryEntry, EntryMetadata, FileKind};
pub use media::{
    is_supported_audio_extension, is_supported_audio_path, is_supported_image_extension,
    is_supported_image_path, is_supported_video_extension, is_supported_video_path,
    supported_media_kind_for_path, SupportedMediaKind,
};
pub use ops::{
    copy_path, copy_path_with_options, create_directory, create_empty_file,
    create_file_with_contents, move_path, move_path_with_options, rename_path, trash_path,
    trash_path_with_restore_entry, CopyProgress, FileOperationControls, FileOperationRunState,
    FileOperationVerification, FileTransferOptions, ProgressSender, TransferConflictStrategy,
};
pub use scan::{scan_directory, DirectoryScan, FileError, ScanOptions, ScanWarning};
pub use search::{
    build_file_search_index, build_file_search_index_for_paths,
    build_file_search_index_for_paths_with_progress, clear_file_search_index_failures,
    file_search_index_exists, file_search_index_status, remove_file_search_index,
    search_file_index, search_file_tree, FileSearchIndexFailure, FileSearchIndexMode,
    FileSearchIndexOptions, FileSearchIndexOutcome, FileSearchIndexProgress, FileSearchIndexStatus,
    FileSearchMatch, FileSearchOptions, FileSearchOutcome,
};
pub use sort::{
    apply_entry_options, compare_entries, filter_hidden, sort_entries, SortDirection, SortField,
};
pub use transfer_conflict::{
    available_transfer_target_path, check_transfer_conflicts, is_transfer_target_available,
    TransferConflictCheck, TransferConflictItem, TransferConflictMetadata,
};
pub use trash_bin::{
    delete_trash_entry, empty_trash, restore_trash_entry, scan_trash, TrashEntry,
    TrashRestoreEntry, TrashScan,
};
pub use watch::{watch_directory, DirectoryChange, DirectoryWatcher};
