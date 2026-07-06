use file_core::{ArchiveCompressionLevel, ArchiveFormat, BatchRenameItem, TrashRestoreEntry};
use file_operation_store::{
    StoredArchiveCompressionLevel, StoredArchiveFormat, StoredBatchRenameItem, StoredOperation,
    StoredPath, StoredTransfer, StoredTrashEntry,
};

use super::{QueuedFileOperation, QueuedTransfer};

pub(super) fn queued_operation_to_stored(operation: &QueuedFileOperation) -> StoredOperation {
    match operation {
        QueuedFileOperation::Rename { path, new_name } => StoredOperation::Rename {
            path: StoredPath::from_path(path),
            new_name: new_name.clone(),
        },
        QueuedFileOperation::BatchRename { items } => StoredOperation::BatchRename {
            items: stored_batch_rename_items(items),
        },
        QueuedFileOperation::CreateDirectory { parent } => StoredOperation::CreateDirectory {
            parent: StoredPath::from_path(parent),
        },
        QueuedFileOperation::CreateEmptyFile { parent } => StoredOperation::CreateEmptyFile {
            parent: StoredPath::from_path(parent),
        },
        QueuedFileOperation::Trash { paths } => StoredOperation::Trash {
            paths: paths
                .iter()
                .map(|path| StoredPath::from_path(path))
                .collect(),
        },
        QueuedFileOperation::Restore { entries } => StoredOperation::Restore {
            entries: stored_trash_entries(entries),
        },
        QueuedFileOperation::DeleteTrashEntries { entries } => {
            StoredOperation::DeleteTrashEntries {
                entries: stored_trash_entries(entries),
            }
        }
        QueuedFileOperation::DeletePermanently { paths } => StoredOperation::DeletePermanently {
            paths: paths
                .iter()
                .map(|path| StoredPath::from_path(path))
                .collect(),
        },
        QueuedFileOperation::EmptyTrash => StoredOperation::EmptyTrash,
        QueuedFileOperation::Copy { transfers, .. } => StoredOperation::Copy {
            transfers: stored_transfers(transfers),
        },
        QueuedFileOperation::Move { transfers, .. } => StoredOperation::Move {
            transfers: stored_transfers(transfers),
        },
        QueuedFileOperation::CreateArchive {
            sources,
            target,
            format,
            compression_level,
            password,
        } => StoredOperation::CreateArchive {
            sources: sources
                .iter()
                .map(|path| StoredPath::from_path(path))
                .collect(),
            target: StoredPath::from_path(target),
            format: stored_archive_format(*format),
            compression_level: stored_archive_compression_level(*compression_level),
            password_required: password.is_some(),
        },
        QueuedFileOperation::ExtractArchive { request } => StoredOperation::ExtractArchive {
            archive: StoredPath::from_path(&request.archive),
            destination: StoredPath::from_path(&request.destination),
            password_required: request.password.is_some(),
        },
    }
}

pub(super) fn queued_operation_from_stored(
    _operation: StoredOperation,
) -> Option<QueuedFileOperation> {
    None
}

fn stored_transfers(transfers: &[QueuedTransfer]) -> Vec<StoredTransfer> {
    transfers
        .iter()
        .map(|transfer| StoredTransfer {
            source: StoredPath::from_path(&transfer.source),
            target: StoredPath::from_path(&transfer.target),
        })
        .collect()
}

fn stored_batch_rename_items(items: &[BatchRenameItem]) -> Vec<StoredBatchRenameItem> {
    items
        .iter()
        .map(|item| StoredBatchRenameItem {
            from: StoredPath::from_path(&item.from),
            to: StoredPath::from_path(&item.to),
        })
        .collect()
}

fn stored_trash_entries(entries: &[TrashRestoreEntry]) -> Vec<StoredTrashEntry> {
    entries
        .iter()
        .map(|entry| StoredTrashEntry {
            trash_path: StoredPath::from_path(&entry.trash_path),
            info_path: StoredPath::from_path(&entry.info_path),
            original_path: StoredPath::from_path(&entry.original_path),
        })
        .collect()
}

fn stored_archive_format(format: ArchiveFormat) -> StoredArchiveFormat {
    match format {
        ArchiveFormat::Zip => StoredArchiveFormat::Zip,
        ArchiveFormat::SevenZip => StoredArchiveFormat::SevenZip,
        ArchiveFormat::TarGz => StoredArchiveFormat::TarGz,
    }
}

fn stored_archive_compression_level(
    compression_level: ArchiveCompressionLevel,
) -> StoredArchiveCompressionLevel {
    match compression_level {
        ArchiveCompressionLevel::Store => StoredArchiveCompressionLevel::Store,
        ArchiveCompressionLevel::Fast => StoredArchiveCompressionLevel::Fast,
        ArchiveCompressionLevel::Balanced => StoredArchiveCompressionLevel::Balanced,
        ArchiveCompressionLevel::Maximum => StoredArchiveCompressionLevel::Maximum,
    }
}
