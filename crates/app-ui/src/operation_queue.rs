use std::path::PathBuf;

use file_core::{
    ArchiveCompressionLevel, ArchiveExtractionRequest, ArchiveFormat, ArchivePassword,
    BatchRenameItem, FileOperationControls, FileOperationRunState, FileOperationVerification,
    TransferConflictStrategy, TrashRestoreEntry,
};
use file_operation_store::{
    RecoverableTaskRunnerLease, StoredOperation, StoredTask, StoredTaskStatus, TaskQueueStore,
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::model::sanitized_application_log_detail;
use crate::operation_progress::{FileOperationProgress, FileOperationProgressUpdate};

mod persistence;
use persistence::{queued_operation_from_stored, queued_operation_to_stored};

pub(crate) const NEW_DIRECTORY_NAME: &str = "New Folder";
pub(crate) const NEW_FILE_NAME: &str = "New File";

const LOCAL_TASK_ID_START: u64 = 1 << 63;

#[cfg(test)]
std::thread_local! {
    static RECORDED_FILE_OPERATION_FAILURES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn record_file_operation_failure(task_id: u64, operation: &str, log_error: &str) {
    tracing::error!(
        target: "app_ui::file_operations",
        event = "file_operation_failed",
        task_id,
        operation,
        error = %log_error,
        "file operation failed"
    );
    #[cfg(test)]
    RECORDED_FILE_OPERATION_FAILURES.with(|count| count.set(count.get() + 1));
}

#[derive(Debug, Clone)]
pub(crate) enum QueuedFileOperation {
    Rename {
        path: PathBuf,
        new_name: String,
    },
    BatchRename {
        items: Vec<BatchRenameItem>,
    },
    CreateDirectory {
        parent: PathBuf,
    },
    CreateEmptyFile {
        parent: PathBuf,
    },
    Trash {
        paths: Vec<PathBuf>,
    },
    Restore {
        entries: Vec<TrashRestoreEntry>,
    },
    DeleteTrashEntries {
        entries: Vec<TrashRestoreEntry>,
    },
    DeletePermanently {
        paths: Vec<PathBuf>,
    },
    EmptyTrash,
    Copy {
        transfers: Vec<QueuedTransfer>,
        verification: FileOperationVerification,
    },
    Move {
        transfers: Vec<QueuedTransfer>,
        verification: FileOperationVerification,
    },
    CreateArchive {
        sources: Vec<PathBuf>,
        target: PathBuf,
        format: ArchiveFormat,
        compression_level: ArchiveCompressionLevel,
        password: Option<ArchivePassword>,
    },
    ExtractArchive {
        request: ArchiveExtractionRequest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueuedTransfer {
    pub(crate) source: PathBuf,
    pub(crate) target: PathBuf,
    pub(crate) conflict_strategy: TransferConflictStrategy,
}

impl QueuedTransfer {
    pub(crate) fn new(source: PathBuf, target: PathBuf) -> Self {
        Self {
            source,
            target,
            conflict_strategy: TransferConflictStrategy::Fail,
        }
    }
}

impl QueuedFileOperation {
    pub(crate) fn title(&self) -> &'static str {
        match self {
            Self::Rename { .. } => "Rename",
            Self::BatchRename { .. } => "Batch Rename",
            Self::CreateDirectory { .. } => "New Folder",
            Self::CreateEmptyFile { .. } => "New File",
            Self::Trash { .. } => "Move to Trash",
            Self::Restore { .. } => "Restore",
            Self::DeleteTrashEntries { .. } => "Delete Permanently",
            Self::DeletePermanently { .. } => "Delete Permanently",
            Self::EmptyTrash => "Empty Trash",
            Self::Copy { .. } => "Copy",
            Self::Move { .. } => "Move",
            Self::CreateArchive { .. } => "Create Archive",
            Self::ExtractArchive { .. } => "Extract Archive",
        }
    }

    pub(crate) fn created_path(&self) -> Option<PathBuf> {
        match self {
            Self::CreateDirectory { parent } => Some(parent.join(NEW_DIRECTORY_NAME)),
            Self::CreateEmptyFile { parent } => Some(parent.join(NEW_FILE_NAME)),
            _ => None,
        }
    }

    pub(crate) fn supports_pause(&self) -> bool {
        !matches!(
            self,
            Self::CreateArchive { .. } | Self::ExtractArchive { .. }
        )
    }

    fn uses_recovery_journal(&self) -> bool {
        matches!(self, Self::Copy { .. } | Self::Move { .. })
    }

    fn to_stored(&self) -> StoredOperation {
        queued_operation_to_stored(self)
    }

    fn from_resumable_stored(operation: StoredOperation) -> Option<Self> {
        queued_operation_from_stored(operation)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileOperationStatus {
    Pending,
    Running,
    Paused,
    Canceling,
    Failed,
    Completed,
    Canceled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileOperationTerminalStatus {
    Completed,
    Failed,
    Canceled,
}

impl FileOperationStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::Canceling => "Canceling",
            Self::Failed => "Failed",
            Self::Completed => "Completed",
            Self::Canceled => "Canceled",
        }
    }

    fn to_stored(self) -> StoredTaskStatus {
        match self {
            Self::Pending => StoredTaskStatus::Pending,
            Self::Running => StoredTaskStatus::Running,
            Self::Paused => StoredTaskStatus::Paused,
            Self::Canceling => StoredTaskStatus::Canceling,
            Self::Failed => StoredTaskStatus::Failed,
            Self::Completed => StoredTaskStatus::Completed,
            Self::Canceled => StoredTaskStatus::Canceled,
        }
    }
}

pub(crate) enum FileOperationFinish {
    Succeeded,
    Canceled,
    Failed(String),
    RecoveryInterrupted(String),
    RecoveryBlocked(String),
}

pub(crate) enum FileOperationEnqueueOutcome {
    Queued { task_id: u64 },
    QueuedWithStorageWarning { task_id: u64, error: String },
    Rejected { error: String },
}

impl FileOperationEnqueueOutcome {
    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        match self {
            Self::Queued { .. } => None,
            Self::QueuedWithStorageWarning { error, .. } | Self::Rejected { error } => Some(error),
        }
    }
}

pub(crate) struct FileOperationTask {
    pub(crate) id: u64,
    pub(crate) operation: QueuedFileOperation,
    pub(crate) status: FileOperationStatus,
    pub(crate) progress: FileOperationProgress,
    pub(crate) error: Option<String>,
    is_read: bool,
    cancel: CancellationToken,
    _runner_lease: Option<RecoverableTaskRunnerLease>,
    run_state_sender: watch::Sender<FileOperationRunState>,
    run_state_receiver: watch::Receiver<FileOperationRunState>,
    is_persisted: bool,
}

pub(crate) struct RunningFileOperation {
    pub(crate) id: u64,
    pub(crate) operation: QueuedFileOperation,
    pub(crate) controls: FileOperationControls,
    pub(crate) store: Option<TaskQueueStore>,
}

pub(crate) struct FileOperationQueue {
    tasks: Vec<FileOperationTask>,
    next_local_id: u64,
    is_panel_open: bool,
    store: Option<TaskQueueStore>,
}

impl FileOperationQueue {
    pub(crate) fn new() -> Self {
        Self {
            tasks: Vec::new(),
            next_local_id: LOCAL_TASK_ID_START,
            is_panel_open: false,
            store: None,
        }
    }

    pub(crate) fn set_store_and_restore(&mut self, store: TaskQueueStore) -> Option<String> {
        let restore_coordinator = match store.try_acquire_recoverable_restore_coordinator() {
            Ok(Some(restore_coordinator)) => restore_coordinator,
            Ok(None) => {
                self.store = Some(store);
                return None;
            }
            Err(error) => {
                self.store = Some(store);
                return Some(storage_error(error));
            }
        };
        let stored_tasks = match store.read_tasks() {
            Ok(stored_tasks) => stored_tasks,
            Err(error) => {
                self.store = Some(store);
                return Some(storage_error(error));
            }
        };
        self.store = Some(store);
        let mut storage_error = None;

        for stored_task in stored_tasks {
            storage_error =
                combine_storage_errors(storage_error, self.restore_stored_task(stored_task));
        }
        drop(restore_coordinator);

        combine_storage_errors(storage_error, self.start_next())
    }

    pub(crate) fn task_queue_store(&self) -> Option<&TaskQueueStore> {
        self.store.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn set_store(&mut self, store: TaskQueueStore) {
        self.store = Some(store);
    }

    pub(crate) fn tasks(&self) -> &[FileOperationTask] {
        &self.tasks
    }

    pub(crate) fn is_panel_open(&self) -> bool {
        self.is_panel_open
    }

    pub(crate) fn open_panel(&mut self) {
        self.is_panel_open = true;
        self.mark_all_read();
    }

    pub(crate) fn close_panel(&mut self) {
        self.is_panel_open = false;
    }

    pub(crate) fn unread_count(&self) -> usize {
        self.tasks.iter().filter(|task| !task.is_read).count()
    }

    pub(crate) fn has_unread_failed_task(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| !task.is_read && task.status == FileOperationStatus::Failed)
    }

    pub(crate) fn enqueue(
        &mut self,
        operation: QueuedFileOperation,
    ) -> FileOperationEnqueueOutcome {
        let stored_operation = operation.to_stored();
        let requires_recovery_journal = operation.uses_recovery_journal();
        let (id, is_persisted, storage_warning, runner_lease) = match &self.store {
            Some(store) if requires_recovery_journal => {
                match store.insert_claimed_recoverable_transfer_task(&stored_operation) {
                    Ok(claimed) => (claimed.task_id, true, None, Some(claimed.runner_lease)),
                    Err(error) => {
                        return FileOperationEnqueueOutcome::Rejected {
                            error: storage_error(error),
                        };
                    }
                }
            }
            Some(store) => match store.insert_task(&stored_operation) {
                Ok(id) => (id, true, None, None),
                Err(error) => (
                    self.allocate_local_id(),
                    false,
                    Some(storage_error(error)),
                    None,
                ),
            },
            None if requires_recovery_journal => {
                return FileOperationEnqueueOutcome::Rejected {
                    error: "File operation queue storage is unavailable; copy and move were not started"
                        .to_owned(),
                };
            }
            None => (self.allocate_local_id(), false, None, None),
        };
        let (run_state_sender, run_state_receiver) = watch::channel(FileOperationRunState::Running);
        let is_read = self.is_panel_open;
        self.tasks.push(FileOperationTask {
            id,
            operation,
            status: FileOperationStatus::Pending,
            progress: FileOperationProgress::pending(),
            error: None,
            is_read,
            cancel: CancellationToken::new(),
            _runner_lease: runner_lease,
            run_state_sender,
            run_state_receiver,
            is_persisted,
        });
        let storage_warning = combine_storage_errors(storage_warning, self.start_next());
        match storage_warning {
            Some(error) => {
                FileOperationEnqueueOutcome::QueuedWithStorageWarning { task_id: id, error }
            }
            None => FileOperationEnqueueOutcome::Queued { task_id: id },
        }
    }

    pub(crate) fn active_subscription(&self) -> Option<RunningFileOperation> {
        self.tasks
            .iter()
            .find(|task| {
                matches!(
                    task.status,
                    FileOperationStatus::Running
                        | FileOperationStatus::Paused
                        | FileOperationStatus::Canceling
                )
            })
            .map(|task| RunningFileOperation {
                id: task.id,
                operation: task.operation.clone(),
                controls: FileOperationControls::new(
                    task.cancel.clone(),
                    task.run_state_receiver.clone(),
                ),
                store: self.store.clone(),
            })
    }

    pub(crate) fn operation(&self, id: u64) -> Option<&QueuedFileOperation> {
        self.tasks
            .iter()
            .find(|task| task.id == id)
            .map(|task| &task.operation)
    }

    pub(crate) fn update_progress(
        &mut self,
        id: u64,
        update: FileOperationProgressUpdate,
    ) -> Option<String> {
        let position = self.tasks.iter().position(|task| task.id == id)?;
        if !matches!(
            self.tasks[position].status,
            FileOperationStatus::Running | FileOperationStatus::Canceling
        ) {
            return None;
        }
        self.tasks[position].progress.update(update);
        None
    }

    pub(crate) fn finish(
        &mut self,
        id: u64,
        outcome: FileOperationFinish,
    ) -> (Option<FileOperationTerminalStatus>, Option<String>) {
        let Some(position) = self.tasks.iter().position(|task| task.id == id) else {
            return (None, None);
        };
        if !matches!(
            self.tasks[position].status,
            FileOperationStatus::Running
                | FileOperationStatus::Paused
                | FileOperationStatus::Canceling
        ) {
            return (None, None);
        }

        let was_canceling = self.tasks[position].status == FileOperationStatus::Canceling;
        let mut storage_error = match outcome {
            FileOperationFinish::Succeeded => {
                let task = &mut self.tasks[position];
                task.status = FileOperationStatus::Completed;
                task.progress.mark_complete();
                task.error = None;
                task.is_read = self.is_panel_open;
                self.persist_task_state(position)
            }
            FileOperationFinish::Canceled => {
                let task = &mut self.tasks[position];
                task.status = FileOperationStatus::Canceled;
                task.error = None;
                task.is_read = self.is_panel_open;
                self.persist_task_state(position)
            }
            FileOperationFinish::Failed(error)
                if was_canceling && !self.tasks[position].operation.uses_recovery_journal() =>
            {
                let task = &mut self.tasks[position];
                drop(error);
                task.status = FileOperationStatus::Canceled;
                task.error = None;
                task.is_read = self.is_panel_open;
                self.persist_task_state(position)
            }
            FileOperationFinish::Failed(error) => {
                let task = &mut self.tasks[position];
                let log_error = sanitized_application_log_detail(&error);
                record_file_operation_failure(id, task.operation.title(), &log_error);
                task.status = FileOperationStatus::Failed;
                task.error = Some(error);
                task.is_read = self.is_panel_open;
                self.persist_task_state(position)
            }
            FileOperationFinish::RecoveryInterrupted(error) => {
                let task = &mut self.tasks[position];
                let log_error = sanitized_application_log_detail(&error);
                record_file_operation_failure(id, task.operation.title(), &log_error);
                task.status = FileOperationStatus::Failed;
                task.error = Some(error);
                task.is_read = self.is_panel_open;
                self.persist_task_state_as(position, StoredTaskStatus::RecoveryPending)
            }
            FileOperationFinish::RecoveryBlocked(error) => {
                let task = &mut self.tasks[position];
                let log_error = sanitized_application_log_detail(&error);
                record_file_operation_failure(id, task.operation.title(), &log_error);
                task.status = FileOperationStatus::Failed;
                task.error = Some(error);
                task.is_read = self.is_panel_open;
                self.persist_task_state_preserving_recovery(position)
            }
        };
        let terminal_status = match self.tasks[position].status {
            FileOperationStatus::Completed => FileOperationTerminalStatus::Completed,
            FileOperationStatus::Failed => FileOperationTerminalStatus::Failed,
            FileOperationStatus::Canceled => FileOperationTerminalStatus::Canceled,
            FileOperationStatus::Pending
            | FileOperationStatus::Running
            | FileOperationStatus::Paused
            | FileOperationStatus::Canceling => {
                unreachable!("file operation finish did not produce a terminal status");
            }
        };

        self.tasks[position]._runner_lease = None;
        storage_error = combine_storage_errors(storage_error, self.start_next());
        (Some(terminal_status), storage_error)
    }

    pub(crate) fn toggle_pause(&mut self, id: u64) -> Option<String> {
        let position = self.tasks.iter().position(|task| task.id == id)?;
        let task = &mut self.tasks[position];
        match task.status {
            FileOperationStatus::Running => {
                let _ = task.run_state_sender.send(FileOperationRunState::Paused);
                task.status = FileOperationStatus::Paused;
                self.persist_task_status(position)
            }
            FileOperationStatus::Paused => {
                let _ = task.run_state_sender.send(FileOperationRunState::Running);
                task.status = FileOperationStatus::Running;
                self.persist_task_status(position)
            }
            _ => None,
        }
    }

    pub(crate) fn cancel(&mut self, id: u64) -> Option<String> {
        let position = self.tasks.iter().position(|task| task.id == id)?;

        let mut storage_error = match self.tasks[position].status {
            FileOperationStatus::Pending
                if self.tasks[position].operation.uses_recovery_journal() =>
            {
                let task = &mut self.tasks[position];
                task.cancel.cancel();
                let _ = task.run_state_sender.send(FileOperationRunState::Running);
                task.status = FileOperationStatus::Canceling;
                task.error = None;
                task.is_read = self.is_panel_open;
                self.persist_task_status(position)
            }
            FileOperationStatus::Pending => {
                let task = &mut self.tasks[position];
                task.status = FileOperationStatus::Canceled;
                task.error = None;
                task.is_read = self.is_panel_open;
                self.persist_task_state(position)
            }
            FileOperationStatus::Running | FileOperationStatus::Paused => {
                let task = &mut self.tasks[position];
                task.cancel.cancel();
                let _ = task.run_state_sender.send(FileOperationRunState::Running);
                task.status = FileOperationStatus::Canceling;
                task.is_read = self.is_panel_open;
                self.persist_task_status(position)
            }
            FileOperationStatus::Canceling
            | FileOperationStatus::Failed
            | FileOperationStatus::Completed
            | FileOperationStatus::Canceled => None,
        };

        storage_error = combine_storage_errors(storage_error, self.start_next());
        storage_error
    }

    pub(crate) fn prepare_for_shutdown(&mut self) -> Option<String> {
        let mut combined_error = None;
        for task in &self.tasks {
            let preserve_recovery = match self.store.as_ref() {
                Some(store) if task.operation.uses_recovery_journal() && task.is_persisted => {
                    match store.read_transfer_recovery(task.id) {
                        Ok(snapshot) => !snapshot.journal_entries.is_empty(),
                        Err(error) => {
                            combined_error =
                                combine_storage_errors(combined_error, Some(storage_error(error)));
                            true
                        }
                    }
                }
                _ => false,
            };
            if preserve_recovery {
                continue;
            }

            task.cancel.cancel();
            let _ = task.run_state_sender.send(FileOperationRunState::Running);
            if task.is_persisted {
                if let Some(store) = self.store.as_ref() {
                    combined_error = combine_storage_errors(
                        combined_error,
                        store.delete_task(task.id).err().map(storage_error),
                    );
                }
            }
        }
        self.tasks.clear();
        self.is_panel_open = false;
        combined_error
    }

    pub(crate) fn task_count(&self) -> usize {
        self.tasks.len()
    }

    pub(crate) fn is_active_paused(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| task.status == FileOperationStatus::Paused)
    }

    pub(crate) fn has_active_task(&self) -> bool {
        self.tasks.iter().any(|task| {
            matches!(
                task.status,
                FileOperationStatus::Running
                    | FileOperationStatus::Paused
                    | FileOperationStatus::Canceling
            )
        })
    }

    pub(crate) fn has_active_indeterminate_progress(&self) -> bool {
        self.tasks.iter().any(|task| {
            matches!(
                task.status,
                FileOperationStatus::Running | FileOperationStatus::Canceling
            ) && task.progress.fraction().is_none()
        })
    }

    pub(crate) fn indicator_progress(&self) -> Option<f32> {
        self.tasks
            .iter()
            .find(|task| {
                matches!(
                    task.status,
                    FileOperationStatus::Running
                        | FileOperationStatus::Paused
                        | FileOperationStatus::Canceling
                )
            })
            .and_then(|task| task.progress.fraction())
    }
}

mod runtime;

fn storage_error(error: impl std::fmt::Display) -> String {
    format!("File operation queue storage failed: {error}")
}

fn combine_storage_errors(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(first), Some(second)) => Some(format!("{first}; {second}")),
        (Some(error), None) | (None, Some(error)) => Some(error),
        (None, None) => None,
    }
}

fn stored_status_is_terminal(status: StoredTaskStatus) -> bool {
    matches!(
        status,
        StoredTaskStatus::Failed | StoredTaskStatus::Completed | StoredTaskStatus::Canceled
    )
}

#[cfg(test)]
mod progress_tests;
#[cfg(test)]
mod tests;
