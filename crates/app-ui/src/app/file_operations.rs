use iced::Task;

use super::{operation_queue_auto_hide_command, FileBrowser};
use crate::model::{Message, OperationQueuePanelMode};
use crate::operation_history::{FileOperationOutcome, PendingHistoryOperation};
use crate::operation_queue::QueuedFileOperation;
use crate::view::rename_input_id;

impl FileBrowser {
    pub(super) fn accept_file_operation_finished(
        &mut self,
        task_id: u64,
        result: Result<FileOperationOutcome, String>,
    ) -> Task<Message> {
        let completed_successfully = result.is_ok();
        let is_history_replay = self.operation_history.is_replaying(task_id);
        let created_path = (completed_successfully && !is_history_replay)
            .then(|| {
                self.operation_queue
                    .operation(task_id)
                    .and_then(QueuedFileOperation::created_path)
            })
            .flatten();

        if completed_successfully {
            self.rename_input.clear();
            if let Some(path) = created_path {
                self.pending_created_entry_rename = Some(path);
            }
        }

        match &result {
            Ok(outcome) => self.operation_history.accept_completed(task_id, outcome),
            Err(_) => self.operation_history.restore_pending(task_id),
        }

        let queue_result = result.as_ref().map(|_| ()).map_err(|error| error.clone());
        let (finished, storage_error) = self.operation_queue.finish(task_id, queue_result);
        if let Some(error) = storage_error {
            self.error = Some(error);
        }

        if finished {
            self.reload_current()
        } else {
            Task::none()
        }
    }

    pub(super) fn commit_rename(&mut self) -> Task<Message> {
        let Some(path) = self.renaming.clone().or_else(|| self.selected.clone()) else {
            return Task::none();
        };

        let name = self.rename_input.trim();
        if name.is_empty() {
            self.renaming = None;
            return Task::none();
        }

        let old_name = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        if old_name == name {
            self.renaming = None;
            return Task::none();
        }

        self.renaming = None;
        self.context_menu = None;
        self.enqueue_file_operation(QueuedFileOperation::Rename {
            path,
            new_name: name.to_owned(),
        })
    }

    pub(super) fn commit_rename_if_active(&mut self) -> Task<Message> {
        if self.renaming.is_some() {
            self.commit_rename()
        } else {
            Task::none()
        }
    }

    pub(super) fn focus_created_entry_for_rename(&mut self) -> Task<Message> {
        let Some(path) = self.pending_created_entry_rename.clone() else {
            return Task::none();
        };
        if self.entry_for_path(&path).is_none() {
            return Task::none();
        }

        self.pending_created_entry_rename = None;
        self.select_path(path.clone());
        self.renaming = Some(path);
        let input_id = rename_input_id();
        Task::batch([
            iced::widget::operation::focus(input_id.clone()),
            iced::widget::operation::select_all(input_id),
        ])
    }

    pub(super) fn enqueue_file_operation(
        &mut self,
        operation: QueuedFileOperation,
    ) -> Task<Message> {
        self.enqueue_file_operation_with_history(operation, None)
    }

    pub(super) fn undo_file_operation(&mut self) -> Task<Message> {
        self.context_menu = None;
        let Some((operation, pending_history)) = self.operation_history.take_undo_operation()
        else {
            return Task::none();
        };
        self.enqueue_file_operation_with_history(operation, Some(pending_history))
    }

    pub(super) fn redo_file_operation(&mut self) -> Task<Message> {
        self.context_menu = None;
        let Some((operation, pending_history)) = self.operation_history.take_redo_operation()
        else {
            return Task::none();
        };
        self.enqueue_file_operation_with_history(operation, Some(pending_history))
    }

    fn enqueue_file_operation_with_history(
        &mut self,
        operation: QueuedFileOperation,
        pending_history: Option<PendingHistoryOperation>,
    ) -> Task<Message> {
        self.error = None;
        if let Some(error) = self.operation_queue.enqueue(operation) {
            self.error = Some(error);
        }
        if let Some(pending_history) = pending_history {
            if let Some(task) = self.operation_queue.tasks().last() {
                self.operation_history
                    .track_pending(task.id, pending_history);
            }
        }
        self.show_operation_queue_temporarily()
    }

    fn show_operation_queue_temporarily(&mut self) -> Task<Message> {
        self.operation_queue_panel_mode = OperationQueuePanelMode::PassivePreview;
        self.operation_queue.open_panel();
        self.operation_queue_auto_hide_generation =
            self.operation_queue_auto_hide_generation.wrapping_add(1);
        operation_queue_auto_hide_command(self.operation_queue_auto_hide_generation)
    }
}
