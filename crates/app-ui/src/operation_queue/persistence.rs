use file_core::{ArchiveCompressionLevel, ArchiveFormat, TrashRestoreEntry};
use file_index::FileSearchIndexMode;
use file_operation_store::{
    StoredArchiveCompressionLevel, StoredArchiveFormat, StoredOperation, StoredPath,
    StoredSearchIndexMode, StoredTransfer, StoredTrashEntry,
};

use super::{QueuedFileOperation, QueuedTransfer};

pub(super) fn queued_operation_to_stored(operation: &QueuedFileOperation) -> StoredOperation {
    match operation {
        QueuedFileOperation::Rename { path, new_name } => StoredOperation::Rename {
            path: StoredPath::from_path(path),
            new_name: new_name.clone(),
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
        QueuedFileOperation::BuildSearchIndex {
            root,
            index_base_dir,
            selected_paths,
            mode,
            ..
        } => StoredOperation::SearchIndex {
            root: StoredPath::from_path(root),
            index_base_dir: StoredPath::from_path(index_base_dir),
            selected_paths: selected_paths
                .iter()
                .map(|path| StoredPath::from_path(path))
                .collect(),
            mode: stored_search_index_mode(*mode),
        },
    }
}

pub(super) fn queued_operation_from_stored(
    operation: StoredOperation,
) -> Option<QueuedFileOperation> {
    match operation {
        StoredOperation::SearchIndex {
            root,
            index_base_dir,
            selected_paths,
            mode,
        } => Some(QueuedFileOperation::BuildSearchIndex {
            profile_id: crate::commands::default_search_profile_id().to_owned(),
            root: root.to_path_buf(),
            index_base_dir: index_base_dir.to_path_buf(),
            selected_paths: selected_paths
                .into_iter()
                .map(|path| path.to_path_buf())
                .collect(),
            mode: file_search_index_mode_from_stored(mode),
        }),
        _ => None,
    }
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

fn stored_search_index_mode(mode: FileSearchIndexMode) -> StoredSearchIndexMode {
    match mode {
        FileSearchIndexMode::FullRebuild => StoredSearchIndexMode::FullRebuild,
        FileSearchIndexMode::Incremental => StoredSearchIndexMode::Incremental,
    }
}

fn file_search_index_mode_from_stored(mode: StoredSearchIndexMode) -> FileSearchIndexMode {
    match mode {
        StoredSearchIndexMode::FullRebuild => FileSearchIndexMode::FullRebuild,
        StoredSearchIndexMode::Incremental => FileSearchIndexMode::Incremental,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn search_index_task_persists_index_base_dir() {
        let operation = QueuedFileOperation::BuildSearchIndex {
            profile_id: "default".to_owned(),
            root: PathBuf::from("/workspace"),
            index_base_dir: PathBuf::from("/cache/file-manager/search-index"),
            selected_paths: vec![PathBuf::from("/workspace")],
            mode: FileSearchIndexMode::Incremental,
        };

        let stored = queued_operation_to_stored(&operation);

        let StoredOperation::SearchIndex { index_base_dir, .. } = stored else {
            panic!("expected search index operation");
        };
        assert_eq!(
            index_base_dir.to_path_buf(),
            PathBuf::from("/cache/file-manager/search-index")
        );
    }
}
