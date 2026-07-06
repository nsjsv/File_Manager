use std::path::{Path, PathBuf};

#[cfg(test)]
use file_core::FileOperationVerification;
use file_core::TrashRestoreEntry;

use crate::operation_queue::{
    QueuedFileOperation, QueuedTransfer, NEW_DIRECTORY_NAME, NEW_FILE_NAME,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileOperationPathLines {
    pub(crate) file_name: String,
    pub(crate) original_path: String,
    pub(crate) directory_path: String,
    pub(crate) total_items: usize,
}

impl QueuedFileOperation {
    pub(crate) fn path_lines(&self) -> FileOperationPathLines {
        match self {
            Self::Rename { path, new_name } => FileOperationPathLines {
                file_name: new_name.clone(),
                original_path: path_full_text(path),
                directory_path: path_parent_text(path),
                total_items: 1,
            },
            Self::BatchRename { items } => match items.as_slice() {
                [] => FileOperationPathLines::empty(),
                [item, ..] => FileOperationPathLines::from_paths(
                    &item.to,
                    &item.from,
                    parent_path(&item.to),
                    items.len(),
                ),
            },
            Self::CreateDirectory { parent } => {
                created_entry_path_lines(parent, NEW_DIRECTORY_NAME)
            }
            Self::CreateEmptyFile { parent } => created_entry_path_lines(parent, NEW_FILE_NAME),
            Self::Trash { paths } | Self::DeletePermanently { paths } => {
                path_lines_from_paths(paths)
            }
            Self::Restore { entries } | Self::DeleteTrashEntries { entries } => {
                path_lines_from_trash_originals(entries)
            }
            Self::EmptyTrash => FileOperationPathLines {
                file_name: "Trash".to_owned(),
                original_path: "Trash".to_owned(),
                directory_path: "Trash".to_owned(),
                total_items: 1,
            },
            Self::Copy { transfers, .. } | Self::Move { transfers, .. } => {
                path_lines_from_transfers(transfers)
            }
            Self::CreateArchive {
                sources, target, ..
            } => path_lines_from_archive(sources, target),
            Self::ExtractArchive { request } => {
                path_lines_from_extracted_archive(&request.archive, &request.destination)
            }
        }
    }
}

impl FileOperationPathLines {
    fn from_paths(
        file_name_path: &Path,
        original_path: &Path,
        directory_path: &Path,
        total_items: usize,
    ) -> Self {
        Self {
            file_name: path_label(file_name_path),
            original_path: path_full_text(original_path),
            directory_path: path_full_text(directory_path),
            total_items,
        }
    }

    fn empty() -> Self {
        Self {
            file_name: "No items".to_owned(),
            original_path: "No items".to_owned(),
            directory_path: "No items".to_owned(),
            total_items: 0,
        }
    }
}

fn created_entry_path_lines(parent: &Path, name: &str) -> FileOperationPathLines {
    let target = parent.join(name);
    FileOperationPathLines::from_paths(&target, &target, parent, 1)
}

fn path_lines_from_paths(paths: &[PathBuf]) -> FileOperationPathLines {
    match paths {
        [] => FileOperationPathLines::empty(),
        [path, ..] => {
            FileOperationPathLines::from_paths(path, path, parent_path(path), paths.len())
        }
    }
}

fn path_lines_from_transfers(transfers: &[QueuedTransfer]) -> FileOperationPathLines {
    match transfers {
        [] => FileOperationPathLines::empty(),
        [transfer, ..] => FileOperationPathLines::from_paths(
            &transfer.target,
            &transfer.source,
            parent_path(&transfer.target),
            transfers.len(),
        ),
    }
}

fn path_lines_from_archive(sources: &[PathBuf], target: &Path) -> FileOperationPathLines {
    match sources {
        [] => FileOperationPathLines::from_paths(target, target, parent_path(target), 1),
        [source, ..] => {
            FileOperationPathLines::from_paths(target, source, parent_path(target), sources.len())
        }
    }
}

fn path_lines_from_extracted_archive(archive: &Path, destination: &Path) -> FileOperationPathLines {
    FileOperationPathLines::from_paths(destination, archive, parent_path(destination), 1)
}

fn path_lines_from_trash_originals(entries: &[TrashRestoreEntry]) -> FileOperationPathLines {
    match entries {
        [] => FileOperationPathLines::empty(),
        [entry, ..] => FileOperationPathLines::from_paths(
            &entry.original_path,
            &entry.original_path,
            parent_path(&entry.original_path),
            entries.len(),
        ),
    }
}

fn path_label(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn path_full_text(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        path.to_string_lossy().into_owned()
    }
}

fn parent_path(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn path_parent_text(path: &Path) -> String {
    path_full_text(parent_path(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_path_lines_expose_name_original_and_directory() {
        let operation = QueuedFileOperation::Copy {
            transfers: vec![QueuedTransfer::new(
                PathBuf::from("/home/user/report.txt"),
                PathBuf::from("/tmp/report.txt"),
            )],
            verification: FileOperationVerification::default(),
        };

        let path_lines = operation.path_lines();

        assert_eq!(path_lines.file_name, "report.txt");
        assert_eq!(path_lines.original_path, "/home/user/report.txt");
        assert_eq!(path_lines.directory_path, "/tmp");
        assert_eq!(path_lines.total_items, 1);
    }
}
