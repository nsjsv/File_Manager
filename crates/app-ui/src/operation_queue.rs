use std::path::PathBuf;

use file_core::{
    ArchiveCompressionLevel, ArchiveExtractionRequest, ArchiveFormat, ArchivePassword,
    FileOperationControls, FileOperationRunState, FileOperationVerification,
    TransferConflictStrategy, TrashRestoreEntry,
};
use file_operation_store::{
    StoredArchiveCompressionLevel, StoredArchiveFormat, StoredOperation, StoredPath,
    StoredProgress, StoredTask, StoredTaskStatus, StoredTransfer, StoredTrashEntry, TaskQueueStore,
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub(crate) const NEW_DIRECTORY_NAME: &str = "New Folder";
pub(crate) const NEW_FILE_NAME: &str = "New File";

const LOCAL_TASK_ID_START: u64 = 1 << 63;
const ACTIVE_TRANSFER_PROGRESS_LIMIT: f32 = 0.999;

#[derive(Debug, Clone)]
pub(crate) enum QueuedFileOperation {
    Rename {
        path: PathBuf,
        new_name: String,
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
    BuildSearchIndex {
        root: PathBuf,
        index_dir: PathBuf,
        selected_paths: Vec<PathBuf>,
        include_hidden: bool,
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
            Self::CreateDirectory { .. } => "New Folder",
            Self::CreateEmptyFile { .. } => "New File",
            Self::Trash { .. } => "Move to Trash",
            Self::Restore { .. } => "Restore",
            Self::DeleteTrashEntries { .. } => "Delete Permanently",
            Self::EmptyTrash => "Empty Trash",
            Self::Copy { .. } => "Copy",
            Self::Move { .. } => "Move",
            Self::CreateArchive { .. } => "Create Archive",
            Self::ExtractArchive { .. } => "Extract Archive",
            Self::BuildSearchIndex { .. } => "Build Search Index",
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
            Self::BuildSearchIndex { .. }
                | Self::CreateArchive { .. }
                | Self::ExtractArchive { .. }
        )
    }

    fn to_stored(&self) -> StoredOperation {
        match self {
            Self::Rename { path, new_name } => StoredOperation::Rename {
                path: StoredPath::from_path(path),
                new_name: new_name.clone(),
            },
            Self::CreateDirectory { parent } => StoredOperation::CreateDirectory {
                parent: StoredPath::from_path(parent),
            },
            Self::CreateEmptyFile { parent } => StoredOperation::CreateEmptyFile {
                parent: StoredPath::from_path(parent),
            },
            Self::Trash { paths } => StoredOperation::Trash {
                paths: paths
                    .iter()
                    .map(|path| StoredPath::from_path(path))
                    .collect(),
            },
            Self::Restore { entries } => StoredOperation::Restore {
                entries: stored_trash_entries(entries),
            },
            Self::DeleteTrashEntries { entries } => StoredOperation::DeleteTrashEntries {
                entries: stored_trash_entries(entries),
            },
            Self::EmptyTrash => StoredOperation::EmptyTrash,
            Self::Copy { transfers, .. } => StoredOperation::Copy {
                transfers: stored_transfers(transfers),
            },
            Self::Move { transfers, .. } => StoredOperation::Move {
                transfers: stored_transfers(transfers),
            },
            Self::CreateArchive {
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
            Self::ExtractArchive { request } => StoredOperation::ExtractArchive {
                archive: StoredPath::from_path(&request.archive),
                destination: StoredPath::from_path(&request.destination),
                password_required: request.password.is_some(),
            },
            Self::BuildSearchIndex {
                root,
                index_dir,
                selected_paths,
                include_hidden,
            } => StoredOperation::SearchIndex {
                root: StoredPath::from_path(root),
                index_dir: StoredPath::from_path(index_dir),
                selected_paths: selected_paths
                    .iter()
                    .map(|path| StoredPath::from_path(path))
                    .collect(),
                include_hidden: *include_hidden,
            },
        }
    }

    fn from_resumable_stored(operation: StoredOperation) -> Option<Self> {
        match operation {
            StoredOperation::SearchIndex {
                root,
                index_dir,
                selected_paths,
                include_hidden,
            } => Some(Self::BuildSearchIndex {
                root: root.to_path_buf(),
                index_dir: index_dir.to_path_buf(),
                selected_paths: selected_paths
                    .into_iter()
                    .map(|path| path.to_path_buf())
                    .collect(),
                include_hidden,
            }),
            _ => None,
        }
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FileOperationProgress {
    fraction: Option<f32>,
}

impl FileOperationProgress {
    pub(crate) fn pending() -> Self {
        Self { fraction: None }
    }

    fn complete() -> Self {
        Self {
            fraction: Some(1.0),
        }
    }

    pub(crate) fn fraction(self) -> Option<f32> {
        self.fraction
    }

    fn to_stored(self) -> StoredProgress {
        match self.fraction {
            Some(fraction) => StoredProgress::with_fraction(fraction as f64),
            None => StoredProgress::pending(),
        }
    }

    fn update(&mut self, update: FileOperationProgressUpdate) {
        let next_fraction = match update {
            FileOperationProgressUpdate::Bytes {
                bytes_done,
                bytes_total,
                completed_transfers,
                total_transfers,
            } if bytes_total > 0 && total_transfers > 0 => {
                let transfer_fraction = (bytes_done as f32 / bytes_total as f32)
                    .clamp(0.0, ACTIVE_TRANSFER_PROGRESS_LIMIT);
                Some((completed_transfers as f32 + transfer_fraction) / total_transfers as f32)
            }
            FileOperationProgressUpdate::Items { completed, total } if total > 0 => {
                Some(completed as f32 / total as f32)
            }
            FileOperationProgressUpdate::Indeterminate => None,
            _ => None,
        };
        self.fraction = match (self.fraction, next_fraction) {
            (Some(current), Some(next)) => Some(current.max(next)),
            (None, Some(next)) => Some(next),
            (current, None) => current,
        }
        .map(|fraction| fraction.clamp(0.0, 1.0));
    }
}

#[derive(Debug, Clone)]
pub(crate) enum FileOperationProgressUpdate {
    Bytes {
        bytes_done: u64,
        bytes_total: u64,
        completed_transfers: usize,
        total_transfers: usize,
    },
    Items {
        completed: usize,
        total: usize,
    },
    Indeterminate,
}

pub(crate) struct FileOperationTask {
    pub(crate) id: u64,
    pub(crate) operation: QueuedFileOperation,
    pub(crate) status: FileOperationStatus,
    pub(crate) progress: FileOperationProgress,
    pub(crate) error: Option<String>,
    is_read: bool,
    cancel: CancellationToken,
    run_state_sender: watch::Sender<FileOperationRunState>,
    run_state_receiver: watch::Receiver<FileOperationRunState>,
    is_persisted: bool,
}

pub(crate) struct RunningFileOperation {
    pub(crate) id: u64,
    pub(crate) operation: QueuedFileOperation,
    pub(crate) controls: FileOperationControls,
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

    pub(crate) fn set_store_and_restore(
        &mut self,
        store: TaskQueueStore,
        stored_tasks: Vec<StoredTask>,
    ) -> Option<String> {
        self.store = Some(store);
        let mut storage_error = None;

        for stored_task in stored_tasks {
            storage_error =
                combine_storage_errors(storage_error, self.restore_stored_task(stored_task));
        }

        combine_storage_errors(storage_error, self.start_next())
    }

    pub(crate) fn task_queue_store(&self) -> Option<&TaskQueueStore> {
        self.store.as_ref()
    }

    #[cfg(test)]
    fn set_store(&mut self, store: TaskQueueStore) {
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

    pub(crate) fn enqueue(&mut self, operation: QueuedFileOperation) -> Option<String> {
        let (id, is_persisted, storage_error) = match &self.store {
            Some(store) => match store.insert_task(&operation.to_stored()) {
                Ok(id) => (id, true, None),
                Err(error) => (self.allocate_local_id(), false, Some(storage_error(error))),
            },
            None => (self.allocate_local_id(), false, None),
        };
        let (run_state_sender, run_state_receiver) = watch::channel(FileOperationRunState::Running);
        self.tasks.push(FileOperationTask {
            id,
            operation,
            status: FileOperationStatus::Pending,
            progress: FileOperationProgress::pending(),
            error: None,
            is_read: false,
            cancel: CancellationToken::new(),
            run_state_sender,
            run_state_receiver,
            is_persisted,
        });
        combine_storage_errors(storage_error, self.start_next())
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
        self.tasks[position].progress.update(update);
        None
    }

    pub(crate) fn finish(&mut self, id: u64, result: Result<(), String>) -> (bool, Option<String>) {
        let Some(position) = self.tasks.iter().position(|task| task.id == id) else {
            return (false, None);
        };

        let was_canceling = self.tasks[position].status == FileOperationStatus::Canceling;
        let mut storage_error = match result {
            Ok(()) => {
                let task = &mut self.tasks[position];
                task.status = FileOperationStatus::Completed;
                task.progress = FileOperationProgress::complete();
                task.error = None;
                task.is_read = self.is_panel_open;
                self.persist_task_state(position)
            }
            Err(error) if was_canceling => {
                let task = &mut self.tasks[position];
                drop(error);
                task.status = FileOperationStatus::Canceled;
                task.error = None;
                task.is_read = self.is_panel_open;
                self.persist_task_state(position)
            }
            Err(error) => {
                let task = &mut self.tasks[position];
                task.status = FileOperationStatus::Failed;
                task.error = Some(error);
                task.is_read = self.is_panel_open;
                self.persist_task_state(position)
            }
        };

        storage_error = combine_storage_errors(storage_error, self.start_next());
        (true, storage_error)
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

    pub(crate) fn cancel_all(&mut self) -> Option<String> {
        for task in &self.tasks {
            task.cancel.cancel();
            let _ = task.run_state_sender.send(FileOperationRunState::Running);
        }
        self.tasks.clear();
        self.is_panel_open = false;
        self.store
            .as_ref()
            .and_then(|store| store.clear_tasks().err().map(storage_error))
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

    fn start_next(&mut self) -> Option<String> {
        if self.active_subscription().is_some() {
            return None;
        }
        if let Some(position) = self
            .tasks
            .iter()
            .find(|task| task.status == FileOperationStatus::Pending)
            .map(|task| task.id)
            .and_then(|id| self.tasks.iter().position(|task| task.id == id))
        {
            let task = &mut self.tasks[position];
            task.status = FileOperationStatus::Running;
            task.progress = FileOperationProgress::pending();
            task.error = None;
            let _ = task.run_state_sender.send(FileOperationRunState::Running);
            return self.persist_task_state(position);
        }
        None
    }

    fn restore_stored_task(&mut self, stored_task: StoredTask) -> Option<String> {
        let StoredTask {
            id,
            operation,
            status,
            progress,
            ..
        } = stored_task;

        if stored_status_is_terminal(status) {
            return None;
        }

        let Some(operation) = QueuedFileOperation::from_resumable_stored(operation) else {
            return self.mark_interrupted_non_resumable_task_failed(id, progress);
        };

        let (run_state_sender, run_state_receiver) = watch::channel(FileOperationRunState::Running);
        self.tasks.push(FileOperationTask {
            id,
            operation,
            status: FileOperationStatus::Pending,
            progress: FileOperationProgress::pending(),
            error: None,
            is_read: false,
            cancel: CancellationToken::new(),
            run_state_sender,
            run_state_receiver,
            is_persisted: true,
        });
        let position = self.tasks.len().saturating_sub(1);
        self.persist_task_state(position)
    }

    fn mark_interrupted_non_resumable_task_failed(
        &self,
        id: u64,
        progress: StoredProgress,
    ) -> Option<String> {
        self.store
            .as_ref()?
            .update_task_state(
                id,
                StoredTaskStatus::Failed,
                progress,
                Some("Task was interrupted and cannot safely resume"),
            )
            .err()
            .map(storage_error)
    }

    fn allocate_local_id(&mut self) -> u64 {
        loop {
            let id = self.next_local_id;
            self.next_local_id = id.checked_add(1).unwrap_or(LOCAL_TASK_ID_START);
            if !self.tasks.iter().any(|task| task.id == id) {
                return id;
            }
        }
    }

    fn mark_all_read(&mut self) {
        for task in &mut self.tasks {
            task.is_read = true;
        }
    }

    fn persist_task_status(&self, position: usize) -> Option<String> {
        let task = &self.tasks[position];
        if !task.is_persisted {
            return None;
        }
        self.store
            .as_ref()?
            .update_status(task.id, task.status.to_stored())
            .err()
            .map(storage_error)
    }

    fn persist_task_state(&self, position: usize) -> Option<String> {
        let task = &self.tasks[position];
        if !task.is_persisted {
            return None;
        }
        self.store
            .as_ref()?
            .update_task_state(
                task.id,
                task.status.to_stored(),
                task.progress.to_stored(),
                task.error.as_deref(),
            )
            .err()
            .map(storage_error)
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
mod tests {
    use super::*;

    fn sample_operation() -> QueuedFileOperation {
        QueuedFileOperation::CreateDirectory {
            parent: PathBuf::from("/tmp"),
        }
    }

    #[test]
    fn local_and_persisted_task_ids_do_not_collide() {
        let mut queue = FileOperationQueue::new();
        queue.enqueue(sample_operation());
        let local_id = queue.tasks()[0].id;

        let root = std::env::temp_dir().join(format!(
            "app-ui-operation-queue-test-{}-{}",
            std::process::id(),
            local_id
        ));
        let store = TaskQueueStore::new(root.join("state.sqlite")).unwrap();
        queue.set_store(store);
        queue.enqueue(sample_operation());

        let persisted_id = queue.tasks()[1].id;
        assert!(local_id > i64::MAX as u64);
        assert!(persisted_id <= i64::MAX as u64);
        assert_ne!(local_id, persisted_id);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn transfer_byte_progress_is_aggregated_by_transfer() {
        let mut progress = FileOperationProgress::pending();

        progress.update(FileOperationProgressUpdate::Bytes {
            bytes_done: 100,
            bytes_total: 100,
            completed_transfers: 0,
            total_transfers: 2,
        });
        assert!(progress.fraction().unwrap() < 0.5);

        progress.update(FileOperationProgressUpdate::Items {
            completed: 1,
            total: 2,
        });
        assert_eq!(progress.fraction(), Some(0.5));

        progress.update(FileOperationProgressUpdate::Bytes {
            bytes_done: 50,
            bytes_total: 100,
            completed_transfers: 1,
            total_transfers: 2,
        });
        let second_transfer_progress = progress.fraction().unwrap();
        assert!(second_transfer_progress > 0.5);
        assert!(second_transfer_progress < 1.0);

        progress.update(FileOperationProgressUpdate::Bytes {
            bytes_done: 100,
            bytes_total: 100,
            completed_transfers: 1,
            total_transfers: 2,
        });
        assert!(progress.fraction().unwrap() < 1.0);

        progress.update(FileOperationProgressUpdate::Items {
            completed: 2,
            total: 2,
        });
        assert_eq!(progress.fraction(), Some(1.0));
    }

    #[test]
    fn finished_tasks_stay_until_queue_is_cleared() {
        let root = std::env::temp_dir().join(format!(
            "app-ui-operation-queue-finish-test-{}",
            std::process::id()
        ));
        let store = TaskQueueStore::new(root.join("state.sqlite")).unwrap();
        let mut queue = FileOperationQueue::new();
        queue.set_store(store.clone());
        queue.enqueue(sample_operation());
        let task_id = queue.tasks()[0].id;

        let (finished, error) = queue.finish(task_id, Ok(()));

        assert!(finished);
        assert!(error.is_none());
        assert_eq!(queue.tasks().len(), 1);
        assert_eq!(queue.tasks()[0].status, FileOperationStatus::Completed);
        assert_eq!(queue.tasks()[0].progress.fraction(), Some(1.0));
        assert!(!queue.has_active_task());
        assert_eq!(queue.unread_count(), 1);
        assert_eq!(
            store.read_task(task_id).unwrap().unwrap().status,
            StoredTaskStatus::Completed
        );

        queue.open_panel();
        assert_eq!(queue.unread_count(), 0);

        assert!(queue.cancel_all().is_none());
        assert!(queue.tasks().is_empty());
        assert!(store.read_tasks().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
