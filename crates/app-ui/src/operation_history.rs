use std::collections::HashMap;
use std::path::{Path, PathBuf};

use file_core::{TransferConflictStrategy, TrashRestoreEntry};

use crate::operation_queue::{QueuedFileOperation, QueuedTransfer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletedTransfer {
    pub(crate) source: PathBuf,
    pub(crate) target: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileOperationOutcome {
    NoHistory,
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    CreateDirectory {
        path: PathBuf,
    },
    CreateEmptyFile {
        path: PathBuf,
    },
    Trash {
        paths: Vec<PathBuf>,
        entries: Vec<TrashRestoreEntry>,
    },
    Restore {
        entries: Vec<TrashRestoreEntry>,
        restored_paths: Vec<PathBuf>,
    },
    Copy {
        transfers: Vec<CompletedTransfer>,
    },
    Move {
        transfers: Vec<CompletedTransfer>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct PendingHistoryOperation {
    direction: HistoryDirection,
    item: FileOperationHistoryItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryDirection {
    Undo,
    Redo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileOperationHistoryItem {
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    CreateDirectory {
        path: PathBuf,
    },
    CreateEmptyFile {
        path: PathBuf,
    },
    Trash {
        original_paths: Vec<PathBuf>,
        restore_entries: Vec<TrashRestoreEntry>,
    },
    Restore {
        restore_entries: Vec<TrashRestoreEntry>,
        restored_paths: Vec<PathBuf>,
    },
    Copy {
        transfers: Vec<CompletedTransfer>,
    },
    Move {
        transfers: Vec<CompletedTransfer>,
    },
}

#[derive(Debug, Default)]
pub(crate) struct FileOperationHistory {
    undo_stack: Vec<FileOperationHistoryItem>,
    redo_stack: Vec<FileOperationHistoryItem>,
    pending_tasks: HashMap<u64, PendingHistoryOperation>,
}

impl FileOperationHistory {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn take_undo_operation(
        &mut self,
    ) -> Option<(QueuedFileOperation, PendingHistoryOperation)> {
        let item = self.undo_stack.pop()?;
        let Some(operation) = item.undo_operation() else {
            self.undo_stack.push(item);
            return None;
        };
        Some((operation, PendingHistoryOperation::undo(item)))
    }

    pub(crate) fn take_redo_operation(
        &mut self,
    ) -> Option<(QueuedFileOperation, PendingHistoryOperation)> {
        let item = self.redo_stack.pop()?;
        let Some(operation) = item.redo_operation() else {
            self.redo_stack.push(item);
            return None;
        };
        Some((operation, PendingHistoryOperation::redo(item)))
    }

    pub(crate) fn track_pending(&mut self, task_id: u64, operation: PendingHistoryOperation) {
        self.pending_tasks.insert(task_id, operation);
    }

    pub(crate) fn is_replaying(&self, task_id: u64) -> bool {
        self.pending_tasks.contains_key(&task_id)
    }

    pub(crate) fn accept_completed(&mut self, task_id: u64, outcome: &FileOperationOutcome) {
        if let Some(pending) = self.pending_tasks.remove(&task_id) {
            self.accept_completed_replay(pending, outcome);
            return;
        }

        if let Some(item) = FileOperationHistoryItem::from_outcome(outcome) {
            self.undo_stack.push(item);
        }
        self.redo_stack.clear();
    }

    pub(crate) fn restore_pending(&mut self, task_id: u64) {
        let Some(pending) = self.pending_tasks.remove(&task_id) else {
            return;
        };

        match pending.direction {
            HistoryDirection::Undo => self.undo_stack.push(pending.item),
            HistoryDirection::Redo => self.redo_stack.push(pending.item),
        }
    }

    fn accept_completed_replay(
        &mut self,
        pending: PendingHistoryOperation,
        outcome: &FileOperationOutcome,
    ) {
        match pending.direction {
            HistoryDirection::Undo => self.redo_stack.push(pending.item.after_undo(outcome)),
            HistoryDirection::Redo => {
                if let Some(item) = FileOperationHistoryItem::from_outcome(outcome) {
                    self.undo_stack.push(item);
                }
            }
        }
    }
}

impl PendingHistoryOperation {
    fn undo(item: FileOperationHistoryItem) -> Self {
        Self {
            direction: HistoryDirection::Undo,
            item,
        }
    }

    fn redo(item: FileOperationHistoryItem) -> Self {
        Self {
            direction: HistoryDirection::Redo,
            item,
        }
    }
}

impl FileOperationHistoryItem {
    fn from_outcome(outcome: &FileOperationOutcome) -> Option<Self> {
        match outcome {
            FileOperationOutcome::NoHistory => None,
            FileOperationOutcome::Rename { from, to } => Some(Self::Rename {
                from: from.clone(),
                to: to.clone(),
            }),
            FileOperationOutcome::CreateDirectory { path } => {
                Some(Self::CreateDirectory { path: path.clone() })
            }
            FileOperationOutcome::CreateEmptyFile { path } => {
                Some(Self::CreateEmptyFile { path: path.clone() })
            }
            FileOperationOutcome::Trash { paths, entries } if paths.len() == entries.len() => {
                Some(Self::Trash {
                    original_paths: paths.clone(),
                    restore_entries: entries.clone(),
                })
            }
            FileOperationOutcome::Trash { .. } => None,
            FileOperationOutcome::Restore {
                entries,
                restored_paths,
            } if !restored_paths.is_empty() => Some(Self::Restore {
                restore_entries: entries.clone(),
                restored_paths: restored_paths.clone(),
            }),
            FileOperationOutcome::Restore { .. } => None,
            FileOperationOutcome::Copy { transfers } if !transfers.is_empty() => Some(Self::Copy {
                transfers: transfers.clone(),
            }),
            FileOperationOutcome::Copy { .. } => None,
            FileOperationOutcome::Move { transfers } if !transfers.is_empty() => Some(Self::Move {
                transfers: transfers.clone(),
            }),
            FileOperationOutcome::Move { .. } => None,
        }
    }

    fn undo_operation(&self) -> Option<QueuedFileOperation> {
        match self {
            Self::Rename { from, to } => rename_operation(to.clone(), from),
            Self::CreateDirectory { path } | Self::CreateEmptyFile { path } => {
                Some(QueuedFileOperation::Trash {
                    paths: vec![path.clone()],
                })
            }
            Self::Trash {
                restore_entries, ..
            } if !restore_entries.is_empty() => Some(QueuedFileOperation::Restore {
                entries: restore_entries.clone(),
            }),
            Self::Trash { .. } => None,
            Self::Restore { restored_paths, .. } => Some(QueuedFileOperation::Trash {
                paths: restored_paths.clone(),
            }),
            Self::Copy { transfers } => Some(QueuedFileOperation::Trash {
                paths: transfer_targets(transfers),
            }),
            Self::Move { transfers } => Some(QueuedFileOperation::Move {
                transfers: reverse_transfers(transfers),
            }),
        }
    }

    fn redo_operation(&self) -> Option<QueuedFileOperation> {
        match self {
            Self::Rename { from, to } => rename_operation(from.clone(), to),
            Self::CreateDirectory { path } => create_directory_operation(path),
            Self::CreateEmptyFile { path } => create_empty_file_operation(path),
            Self::Trash { original_paths, .. } => Some(QueuedFileOperation::Trash {
                paths: original_paths.clone(),
            }),
            Self::Restore {
                restore_entries, ..
            } if !restore_entries.is_empty() => Some(QueuedFileOperation::Restore {
                entries: restore_entries.clone(),
            }),
            Self::Restore { .. } => None,
            Self::Copy { transfers } => Some(QueuedFileOperation::Copy {
                transfers: forward_transfers(transfers),
            }),
            Self::Move { transfers } => Some(QueuedFileOperation::Move {
                transfers: forward_transfers(transfers),
            }),
        }
    }

    fn after_undo(self, outcome: &FileOperationOutcome) -> Self {
        match (self, outcome) {
            (Self::Trash { .. }, FileOperationOutcome::Restore { restored_paths, .. }) => {
                Self::Trash {
                    original_paths: restored_paths.clone(),
                    restore_entries: Vec::new(),
                }
            }
            (Self::Restore { restored_paths, .. }, FileOperationOutcome::Trash { entries, .. }) => {
                Self::Restore {
                    restore_entries: entries.clone(),
                    restored_paths,
                }
            }
            (item, _) => item,
        }
    }
}

fn rename_operation(path: PathBuf, target: &Path) -> Option<QueuedFileOperation> {
    target.file_name().map(|name| QueuedFileOperation::Rename {
        path,
        new_name: name.to_string_lossy().into_owned(),
    })
}

fn create_directory_operation(path: &Path) -> Option<QueuedFileOperation> {
    path.parent()
        .map(Path::to_path_buf)
        .map(|parent| QueuedFileOperation::CreateDirectory { parent })
}

fn create_empty_file_operation(path: &Path) -> Option<QueuedFileOperation> {
    path.parent()
        .map(Path::to_path_buf)
        .map(|parent| QueuedFileOperation::CreateEmptyFile { parent })
}

fn forward_transfers(transfers: &[CompletedTransfer]) -> Vec<QueuedTransfer> {
    transfers
        .iter()
        .map(|transfer| QueuedTransfer {
            source: transfer.source.clone(),
            target: transfer.target.clone(),
            conflict_strategy: TransferConflictStrategy::Fail,
        })
        .collect()
}

fn reverse_transfers(transfers: &[CompletedTransfer]) -> Vec<QueuedTransfer> {
    transfers
        .iter()
        .rev()
        .map(|transfer| QueuedTransfer {
            source: transfer.target.clone(),
            target: transfer.source.clone(),
            conflict_strategy: TransferConflictStrategy::Fail,
        })
        .collect()
}

fn transfer_targets(transfers: &[CompletedTransfer]) -> Vec<PathBuf> {
    transfers
        .iter()
        .rev()
        .map(|transfer| transfer.target.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transfer(source: &str, target: &str) -> CompletedTransfer {
        CompletedTransfer {
            source: PathBuf::from(source),
            target: PathBuf::from(target),
        }
    }

    #[test]
    fn undo_move_reverses_completed_transfers() {
        let item = FileOperationHistoryItem::Move {
            transfers: vec![transfer("/a/one", "/b/one"), transfer("/a/two", "/b/two")],
        };

        let operation = item.undo_operation().expect("undo operation");

        let QueuedFileOperation::Move { transfers } = operation else {
            panic!("expected move");
        };
        assert_eq!(transfers[0].source, PathBuf::from("/b/two"));
        assert_eq!(transfers[0].target, PathBuf::from("/a/two"));
        assert_eq!(transfers[1].source, PathBuf::from("/b/one"));
        assert_eq!(transfers[1].target, PathBuf::from("/a/one"));
    }

    #[test]
    fn normal_operation_clears_redo_stack() {
        let mut history = FileOperationHistory::new();
        history
            .undo_stack
            .push(FileOperationHistoryItem::CreateEmptyFile {
                path: PathBuf::from("/tmp/New File"),
            });
        let (_operation, pending) = history.take_undo_operation().expect("undo");
        history.track_pending(1, pending);
        history.accept_completed(
            1,
            &FileOperationOutcome::Trash {
                paths: vec![PathBuf::from("/tmp/New File")],
                entries: Vec::new(),
            },
        );
        assert_eq!(history.redo_stack.len(), 1);

        history.accept_completed(
            2,
            &FileOperationOutcome::CreateDirectory {
                path: PathBuf::from("/tmp/New Folder"),
            },
        );

        assert!(history.redo_stack.is_empty());
        assert_eq!(history.undo_stack.len(), 1);
    }
}
