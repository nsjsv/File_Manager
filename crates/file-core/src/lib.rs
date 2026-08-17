pub mod archive;
pub mod archive_extraction;
pub mod archive_listing;
pub mod directory_metadata;
pub mod entry;
pub mod media;
pub mod mount_table;
pub mod ops;
pub mod scan;
pub mod sort;
pub mod transfer_conflict;
pub mod trash_bin;
pub mod watch;

pub use archive::{
    create_archive_with_controls_and_progress, create_archive_with_progress,
    ArchiveCompressionLevel, ArchiveCreationProgress, ArchiveCreationRequest, ArchiveFormat,
    ArchivePassword,
};
pub use archive_extraction::{
    archive_extraction_format_for_path, extract_archive,
    extract_archive_with_controls_and_progress, extract_archive_with_progress,
    inspect_archive_extraction, is_supported_archive_path, ArchiveExtractionFormat,
    ArchiveExtractionProgress, ArchiveExtractionRequest,
};
pub use archive_listing::{list_archive_members, ArchiveListingEntry};
pub use directory_metadata::{
    resolve_directory_metadata, DirectoryFilesystemMetadata, DirectoryIdentityNames,
    DirectoryMetadataRequest, DirectoryMetadataRequirement, DirectoryMetadataResolution,
    DirectoryMetadataResolver, DirectoryMetadataState, DirectoryMetadataUnavailable,
    DiscoveredDirectoryEntry,
};
pub use entry::{DirectoryEntry, DirectoryMetadataAvailability, EntryMetadata, FileKind};
pub use media::{
    is_supported_audio_extension, is_supported_audio_path, is_supported_image_extension,
    is_supported_image_path, is_supported_video_extension, is_supported_video_path,
    supported_media_kind_for_path, SupportedMediaKind,
};
pub use ops::{
    batch_rename_paths, copy_path, copy_path_with_options, create_directory, create_empty_file,
    create_file_with_contents, delete_path_permanently, move_path, move_path_with_options,
    persist_recoverable_source_manifest, persist_recoverable_source_manifest_with_controls,
    rename_path, run_recoverable_transfer, ArtifactOwner, ArtifactToken, BackupCreationTransfer,
    BatchRenameItem, CommitPayload, CommitTransfer, CommittedTransfer, CompletedBatchRename,
    CompletedTarget, CopyProgress, FileIdentity, FileObjectKind, FileOperationControls,
    FileOperationRunState, FileOperationVerification, FileTransferOptions, MergeChildCompletion,
    MergeChildOutcome, MergeTransfer, ObjectFingerprint, OwnedArtifact, OwnedArtifactKind,
    OwnedArtifactPlan, PreparedTransfer, ProgressSender, RecoverableTransferError,
    RecoverableTransferOperation, RecoverableTransferOutcome, RecoverableTransferRequest,
    RetiredSource, SourceDisposition, SourceManifest, SourceManifestEntry, SourceRetirementPlan,
    StagedSourceLocation, StagingTransfer, TransferCheckpoint, TransferConflictStrategy,
    TransferExecutionKind, TransferJournal, TransferJournalError, TransferJournalFuture,
    TransferJournalMutation, TransferJournalRecord, TransferWorkKey,
};
pub use scan::{
    discover_directory_with_progress, scan_directory, scan_directory_with_progress,
    DirectoryDiscovery, DirectoryDiscoveryBatch, DirectoryScan, DirectoryScanBatch, FileError,
    ScanOptions, ScanWarning,
};
pub use sort::{
    apply_entry_options, compare_entries, discovered_sort_is_ready, filter_hidden,
    sort_discovered_entry_indices, sort_entries, SortDirection, SortField,
};
pub use transfer_conflict::{
    available_transfer_target_path, check_transfer_conflicts, is_transfer_target_available,
    TransferConflictCheck, TransferConflictItem, TransferConflictMetadata,
};
pub use trash_bin::{
    delete_trash_entry, empty_trash, empty_trash_with_cancellation, restore_trash_entry,
    scan_trash, scan_trash_with_cancellation, trash_path, trash_path_with_restore_entry,
    trash_path_with_restore_entry_and_cancellation, TrashCommitOutcome, TrashEntry,
    TrashRestoreEntry, TrashScan, TrashTrackingWarning,
};
pub use watch::{watch_directory, DirectoryChange, DirectoryWatcher};

pub(crate) const SEVEN_ZIP_COMMAND_NAMES: [&str; 3] = ["7z", "7zz", "7za"];
