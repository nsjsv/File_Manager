use std::collections::HashMap;
use std::path::{Path, PathBuf};

use file_core::{
    BatchRenameItem, CompletedBatchRename, FileOperationVerification, TransferConflictStrategy,
    TrashRestoreEntry,
};

use crate::operation_queue::{QueuedFileOperation, QueuedTransfer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletedTransfer {
    pub(crate) source: PathBuf,
    pub(crate) target: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileOperationHistoryEligibility {
    Replayable,
    NotReplayable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletedPathMigration {
    source: PathBuf,
    destination: PathBuf,
}

impl CompletedPathMigration {
    pub(crate) fn new(source: PathBuf, destination: PathBuf) -> Self {
        Self {
            source,
            destination,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileOperationOutcome {
    NoHistory,
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    BatchRename {
        renames: Vec<CompletedBatchRename>,
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
        history_eligibility: FileOperationHistoryEligibility,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileOperationCompletion {
    Succeeded(FileOperationOutcome),
    Failed {
        error: String,
        completed_move_transfers: Vec<CompletedTransfer>,
    },
    Canceled(Vec<CompletedTransfer>),
    RecoveryInterrupted(String, Vec<CompletedTransfer>),
    RecoveryBlocked {
        error: String,
        completed_move_transfers: Vec<CompletedTransfer>,
    },
}

impl FileOperationCompletion {
    pub(crate) fn from_result(result: Result<FileOperationOutcome, String>) -> Self {
        match result {
            Ok(outcome) => Self::Succeeded(outcome),
            Err(error) => Self::Failed {
                error,
                completed_move_transfers: Vec::new(),
            },
        }
    }

    pub(crate) fn failed_after_completed_moves(
        error: String,
        completed_move_transfers: Vec<CompletedTransfer>,
    ) -> Self {
        Self::Failed {
            error,
            completed_move_transfers,
        }
    }

    pub(crate) fn completed_path_migrations(&self) -> Vec<CompletedPathMigration> {
        match self {
            Self::Succeeded(outcome) => outcome.completed_path_migrations(),
            Self::Failed {
                completed_move_transfers,
                ..
            }
            | Self::Canceled(completed_move_transfers)
            | Self::RecoveryInterrupted(_, completed_move_transfers)
            | Self::RecoveryBlocked {
                completed_move_transfers,
                ..
            } => completed_move_transfers
                .iter()
                .map(|transfer| {
                    CompletedPathMigration::new(transfer.source.clone(), transfer.target.clone())
                })
                .collect(),
        }
    }
}

impl FileOperationOutcome {
    pub(crate) fn completed_path_migrations(&self) -> Vec<CompletedPathMigration> {
        match self {
            Self::Rename { from, to } => {
                vec![CompletedPathMigration::new(from.clone(), to.clone())]
            }
            Self::BatchRename { renames } => renames
                .iter()
                .map(|rename| CompletedPathMigration::new(rename.from.clone(), rename.to.clone()))
                .collect(),
            Self::Move { transfers, .. } => transfers
                .iter()
                .map(|transfer| {
                    CompletedPathMigration::new(transfer.source.clone(), transfer.target.clone())
                })
                .collect(),
            Self::NoHistory
            | Self::CreateDirectory { .. }
            | Self::CreateEmptyFile { .. }
            | Self::Trash { .. }
            | Self::Restore { .. }
            | Self::Copy { .. } => Vec::new(),
        }
    }
}

pub(crate) fn path_after_completed_migrations(
    original_path: &Path,
    migrations: &[CompletedPathMigration],
) -> PathBuf {
    // ponytail: 交互式批次和浏览状态规模有限；若支持超大批次，再升级为组件前缀索引。
    migrations
        .iter()
        .filter_map(|migration| {
            let relative_suffix = original_path.strip_prefix(&migration.source).ok()?;
            Some((
                migration.source.components().count(),
                migration.destination.join(relative_suffix),
            ))
        })
        .max_by_key(|(source_component_count, _)| *source_component_count)
        .map(|(_, migrated_path)| migrated_path)
        .unwrap_or_else(|| original_path.to_path_buf())
}

pub(crate) fn completed_migrations_touch_directory_tree(
    directory: &Path,
    migrations: &[CompletedPathMigration],
) -> bool {
    migrations.iter().any(|migration| {
        migration.source.strip_prefix(directory).is_ok()
            || migration.destination.strip_prefix(directory).is_ok()
    })
}

pub(crate) fn completed_migrations_cross_directory_tree_boundary(
    directory: &Path,
    migrations: &[CompletedPathMigration],
) -> bool {
    migrations.iter().any(|migration| {
        let source_is_inside = migration.source.strip_prefix(directory).is_ok();
        let destination_is_inside = migration.destination.strip_prefix(directory).is_ok();
        source_is_inside != destination_is_inside
    })
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
    BatchRename {
        renames: Vec<CompletedBatchRename>,
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

    pub(crate) fn reject_pending(&mut self, pending: PendingHistoryOperation) {
        self.restore_item(pending.direction, pending.item);
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

    pub(crate) fn accept_failed(
        &mut self,
        task_id: u64,
        completed_move_transfers: &[CompletedTransfer],
    ) {
        let Some(pending) = self.pending_tasks.remove(&task_id) else {
            return;
        };

        let PendingHistoryOperation { direction, item } = pending;
        let FileOperationHistoryItem::Move { transfers } = item else {
            self.restore_item(direction, item);
            return;
        };
        if completed_move_transfers.is_empty() {
            self.restore_item(direction, FileOperationHistoryItem::Move { transfers });
            return;
        }

        let completed_original_transfers = match direction {
            HistoryDirection::Undo => completed_move_transfers
                .iter()
                .rev()
                .map(|transfer| CompletedTransfer {
                    source: transfer.target.clone(),
                    target: transfer.source.clone(),
                })
                .collect::<Vec<_>>(),
            HistoryDirection::Redo => completed_move_transfers.to_vec(),
        };
        let mut unmatched_completed_transfers = completed_original_transfers.clone();
        let remaining_original_transfers = transfers
            .into_iter()
            .filter(|transfer| {
                let Some(completed_index) = unmatched_completed_transfers
                    .iter()
                    .position(|completed_transfer| completed_transfer == transfer)
                else {
                    return true;
                };
                unmatched_completed_transfers.remove(completed_index);
                false
            })
            .collect::<Vec<_>>();
        debug_assert!(
            unmatched_completed_transfers.is_empty(),
            "completed history replay transfers must belong to the pending move"
        );

        match direction {
            HistoryDirection::Undo => {
                self.push_move_item(HistoryDirection::Undo, remaining_original_transfers);
                self.push_move_item(HistoryDirection::Redo, completed_original_transfers);
            }
            HistoryDirection::Redo => {
                self.push_move_item(HistoryDirection::Redo, remaining_original_transfers);
                self.push_move_item(HistoryDirection::Undo, completed_original_transfers);
            }
        }
    }

    fn restore_item(&mut self, direction: HistoryDirection, item: FileOperationHistoryItem) {
        match direction {
            HistoryDirection::Undo => self.undo_stack.push(item),
            HistoryDirection::Redo => self.redo_stack.push(item),
        }
    }

    fn push_move_item(&mut self, direction: HistoryDirection, transfers: Vec<CompletedTransfer>) {
        if !transfers.is_empty() {
            self.restore_item(direction, FileOperationHistoryItem::Move { transfers });
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
            FileOperationOutcome::BatchRename { renames } if !renames.is_empty() => {
                Some(Self::BatchRename {
                    renames: renames.clone(),
                })
            }
            FileOperationOutcome::BatchRename { .. } => None,
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
            FileOperationOutcome::Move {
                transfers,
                history_eligibility: FileOperationHistoryEligibility::Replayable,
            } if !transfers.is_empty() => Some(Self::Move {
                transfers: transfers.clone(),
            }),
            FileOperationOutcome::Move { .. } => None,
        }
    }

    fn undo_operation(&self) -> Option<QueuedFileOperation> {
        match self {
            Self::Rename { from, to } => rename_operation(to.clone(), from),
            Self::BatchRename { renames } => Some(QueuedFileOperation::BatchRename {
                items: reverse_batch_rename_items(renames),
            }),
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
                verification: FileOperationVerification::default(),
            }),
        }
    }

    fn redo_operation(&self) -> Option<QueuedFileOperation> {
        match self {
            Self::Rename { from, to } => rename_operation(from.clone(), to),
            Self::BatchRename { renames } => Some(QueuedFileOperation::BatchRename {
                items: forward_batch_rename_items(renames),
            }),
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
                verification: FileOperationVerification::default(),
            }),
            Self::Move { transfers } => Some(QueuedFileOperation::Move {
                transfers: forward_transfers(transfers),
                verification: FileOperationVerification::default(),
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

fn forward_batch_rename_items(renames: &[CompletedBatchRename]) -> Vec<BatchRenameItem> {
    renames
        .iter()
        .map(|rename| BatchRenameItem {
            from: rename.from.clone(),
            to: rename.to.clone(),
        })
        .collect()
}

fn reverse_batch_rename_items(renames: &[CompletedBatchRename]) -> Vec<BatchRenameItem> {
    renames
        .iter()
        .rev()
        .map(|rename| BatchRenameItem {
            from: rename.to.clone(),
            to: rename.from.clone(),
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
    fn completed_path_migrations_are_component_aware_longest_and_single_pass() {
        let outcome = FileOperationOutcome::BatchRename {
            renames: vec![
                CompletedBatchRename {
                    from: PathBuf::from("/workspace/old"),
                    to: PathBuf::from("/workspace/new"),
                },
                CompletedBatchRename {
                    from: PathBuf::from("/workspace/new"),
                    to: PathBuf::from("/workspace/final"),
                },
                CompletedBatchRename {
                    from: PathBuf::from("/workspace/old/nested"),
                    to: PathBuf::from("/workspace/special"),
                },
            ],
        };
        let migrations = outcome.completed_path_migrations();

        assert_eq!(
            path_after_completed_migrations(Path::new("/workspace/old"), &migrations),
            PathBuf::from("/workspace/new")
        );
        assert_eq!(
            path_after_completed_migrations(Path::new("/workspace/old/file.txt"), &migrations),
            PathBuf::from("/workspace/new/file.txt")
        );
        assert_eq!(
            path_after_completed_migrations(Path::new("/workspace/old-copy"), &migrations),
            PathBuf::from("/workspace/old-copy")
        );
        assert_eq!(
            path_after_completed_migrations(
                Path::new("/workspace/old/nested/file.txt"),
                &migrations,
            ),
            PathBuf::from("/workspace/special/file.txt")
        );
        assert_eq!(
            path_after_completed_migrations(Path::new("/workspace/new/file.txt"), &migrations),
            PathBuf::from("/workspace/final/file.txt")
        );
    }

    #[test]
    fn non_replayable_move_still_reports_completed_path_migration() {
        let outcome = FileOperationOutcome::Move {
            transfers: vec![transfer("/workspace/old", "/archive/old")],
            history_eligibility: FileOperationHistoryEligibility::NotReplayable,
        };
        let migrations = outcome.completed_path_migrations();
        let mut history = FileOperationHistory::new();

        history.accept_completed(1, &outcome);

        assert_eq!(
            path_after_completed_migrations(Path::new("/workspace/old/child"), &migrations),
            PathBuf::from("/archive/old/child")
        );
        assert!(history.take_undo_operation().is_none());
    }

    #[test]
    fn undo_move_reverses_completed_transfers() {
        let item = FileOperationHistoryItem::Move {
            transfers: vec![transfer("/a/one", "/b/one"), transfer("/a/two", "/b/two")],
        };

        let operation = item.undo_operation().expect("undo operation");

        let QueuedFileOperation::Move { transfers, .. } = operation else {
            panic!("expected move");
        };
        assert_eq!(transfers[0].source, PathBuf::from("/b/two"));
        assert_eq!(transfers[0].target, PathBuf::from("/a/two"));
        assert_eq!(transfers[1].source, PathBuf::from("/b/one"));
        assert_eq!(transfers[1].target, PathBuf::from("/a/one"));
    }

    #[test]
    fn failed_undo_move_splits_completed_and_remaining_transfers() {
        let mut history = FileOperationHistory::new();
        history.undo_stack.push(FileOperationHistoryItem::Move {
            transfers: vec![transfer("/a/one", "/b/one"), transfer("/a/two", "/b/two")],
        });
        let (_operation, pending) = history.take_undo_operation().expect("undo move");
        history.track_pending(1, pending);

        history.accept_failed(1, &[transfer("/b/two", "/a/two")]);

        let (remaining_undo, _) = history
            .take_undo_operation()
            .expect("remaining undo transfer");
        let QueuedFileOperation::Move {
            transfers: remaining_undo_transfers,
            ..
        } = remaining_undo
        else {
            panic!("expected remaining undo move");
        };
        assert_eq!(remaining_undo_transfers.len(), 1);
        assert_eq!(remaining_undo_transfers[0].source, PathBuf::from("/b/one"));
        assert_eq!(remaining_undo_transfers[0].target, PathBuf::from("/a/one"));

        let (completed_redo, _) = history
            .take_redo_operation()
            .expect("completed transfer redo");
        let QueuedFileOperation::Move {
            transfers: completed_redo_transfers,
            ..
        } = completed_redo
        else {
            panic!("expected completed transfer redo move");
        };
        assert_eq!(completed_redo_transfers.len(), 1);
        assert_eq!(completed_redo_transfers[0].source, PathBuf::from("/a/two"));
        assert_eq!(completed_redo_transfers[0].target, PathBuf::from("/b/two"));
    }

    #[test]
    fn failed_redo_move_splits_completed_and_remaining_transfers() {
        let mut history = FileOperationHistory::new();
        history.redo_stack.push(FileOperationHistoryItem::Move {
            transfers: vec![transfer("/a/one", "/b/one"), transfer("/a/two", "/b/two")],
        });
        let (_operation, pending) = history.take_redo_operation().expect("redo move");
        history.track_pending(1, pending);

        history.accept_failed(1, &[transfer("/a/one", "/b/one")]);

        let (remaining_redo, _) = history
            .take_redo_operation()
            .expect("remaining redo transfer");
        let QueuedFileOperation::Move {
            transfers: remaining_redo_transfers,
            ..
        } = remaining_redo
        else {
            panic!("expected remaining redo move");
        };
        assert_eq!(remaining_redo_transfers.len(), 1);
        assert_eq!(remaining_redo_transfers[0].source, PathBuf::from("/a/two"));
        assert_eq!(remaining_redo_transfers[0].target, PathBuf::from("/b/two"));

        let (completed_undo, _) = history
            .take_undo_operation()
            .expect("completed transfer undo");
        let QueuedFileOperation::Move {
            transfers: completed_undo_transfers,
            ..
        } = completed_undo
        else {
            panic!("expected completed transfer undo move");
        };
        assert_eq!(completed_undo_transfers.len(), 1);
        assert_eq!(completed_undo_transfers[0].source, PathBuf::from("/b/one"));
        assert_eq!(completed_undo_transfers[0].target, PathBuf::from("/a/one"));
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
