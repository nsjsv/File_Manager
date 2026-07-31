use iced::Task;
use std::path::PathBuf;

use super::FileBrowser;
use crate::model::Message;
use crate::operation_history::{
    path_after_completed_migrations, FileOperationCompletion, PendingHistoryOperation,
};
use crate::operation_queue::{
    FileOperationEnqueueOutcome, FileOperationFinish, QueuedFileOperation,
};
use crate::view::rename_input_id;

// ponytail: 重命名会话短且输入有限，完整字符串快照的内存上限随编辑次数和名称长度增长；若支持长文本或长期会话，再升级为合并编辑事务。
#[derive(Debug, Default)]
pub(super) struct RenameInputHistory {
    undo_values: Vec<String>,
    redo_values: Vec<String>,
}

impl RenameInputHistory {
    fn apply_input_change(&mut self, current_value: &mut String, next_value: String) {
        if current_value == &next_value {
            return;
        }

        self.undo_values
            .push(std::mem::replace(current_value, next_value));
        self.redo_values.clear();
    }

    fn undo(&mut self, current_value: &mut String) {
        let Some(previous_value) = self.undo_values.pop() else {
            return;
        };

        self.redo_values
            .push(std::mem::replace(current_value, previous_value));
    }

    fn redo(&mut self, current_value: &mut String) {
        let Some(next_value) = self.redo_values.pop() else {
            return;
        };

        self.undo_values
            .push(std::mem::replace(current_value, next_value));
    }

    fn reset(&mut self) {
        self.undo_values.clear();
        self.redo_values.clear();
    }
}

impl FileBrowser {
    pub(super) fn accept_file_operation_finished(
        &mut self,
        task_id: u64,
        completion: FileOperationCompletion,
    ) -> Task<Message> {
        let completed_operation = self.operation_queue.operation(task_id).cloned();
        let queue_outcome = match &completion {
            FileOperationCompletion::Succeeded(_) => FileOperationFinish::Succeeded,
            FileOperationCompletion::Canceled(_) => FileOperationFinish::Canceled,
            FileOperationCompletion::Failed { error, .. } => {
                FileOperationFinish::Failed(error.clone())
            }
            FileOperationCompletion::RecoveryInterrupted(error, _) => {
                FileOperationFinish::RecoveryInterrupted(error.clone())
            }
            FileOperationCompletion::RecoveryBlocked { error, .. } => {
                FileOperationFinish::RecoveryBlocked(error.clone())
            }
        };
        let (terminal_status, storage_error) = self.operation_queue.finish(task_id, queue_outcome);
        if let Some(error) = storage_error {
            self.show_global_error(error);
        }
        let Some(terminal_status) = terminal_status else {
            return Task::none();
        };

        let completed_successfully = matches!(completion, FileOperationCompletion::Succeeded(_));
        let is_history_replay = self.operation_history.is_replaying(task_id);
        let created_path = (completed_successfully && !is_history_replay)
            .then(|| {
                completed_operation
                    .as_ref()
                    .and_then(QueuedFileOperation::created_path)
            })
            .flatten();

        if let Some(path) = created_path {
            self.pending_created_entry_rename = Some(path);
        }

        let desktop_notification_task = match completed_operation.as_ref() {
            Some(operation) => {
                self.file_operation_notification_command(operation, terminal_status, &completion)
            }
            None => Task::none(),
        };
        let search_refresh_task = self.migrate_paths_after_file_operation(&completion);
        match &completion {
            FileOperationCompletion::Succeeded(outcome) => {
                self.operation_history.accept_completed(task_id, outcome);
            }
            FileOperationCompletion::Canceled(completed_move_transfers)
            | FileOperationCompletion::Failed {
                completed_move_transfers,
                ..
            }
            | FileOperationCompletion::RecoveryInterrupted(_, completed_move_transfers)
            | FileOperationCompletion::RecoveryBlocked {
                completed_move_transfers,
                ..
            } => self
                .operation_history
                .accept_failed(task_id, completed_move_transfers),
        }

        let pane_reload_task = if let Some(operation) = completed_operation.as_ref() {
            self.invalidate_list_directory_summaries_for_file_operation(operation);
            self.reload_visible_panes_after_file_operation_preserving_list_directory_summaries()
        } else {
            self.reload_visible_panes_after_file_operation()
        };
        Task::batch([
            desktop_notification_task,
            search_refresh_task,
            pane_reload_task,
        ])
    }

    fn migrate_paths_after_file_operation(
        &mut self,
        completion: &FileOperationCompletion,
    ) -> Task<Message> {
        let migrations = completion.completed_path_migrations();
        if migrations.is_empty() {
            return Task::none();
        }

        self.sync_active_tab_state();
        for pane in &mut self.panes {
            pane.sync_active_tab_state();
            pane.migrate_completed_paths(&migrations);
        }
        if let Some(active_pane) = self.pane_by_id(self.active_pane_id()).cloned() {
            self.apply_pane_browsing_snapshot(active_pane);
        }
        self.column_return_targets = self
            .column_return_targets
            .drain()
            .map(|(directory, target)| {
                (
                    path_after_completed_migrations(&directory, &migrations),
                    path_after_completed_migrations(&target, &migrations),
                )
            })
            .collect();
        if let Some(path) = &mut self.pending_created_entry_rename {
            *path = path_after_completed_migrations(path, &migrations);
        }
        if let Some(path) = &mut self.renaming {
            *path = path_after_completed_migrations(path, &migrations);
        }
        if let Some(address_editing) = &mut self.address_editing {
            for suggestion in &mut address_editing.suggestions {
                *suggestion = path_after_completed_migrations(suggestion, &migrations);
            }
        }

        if self.search.is_active() && !self.search.input.trim().is_empty() {
            self.submit_search()
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

    pub(super) fn begin_rename(&mut self, path: PathBuf) -> Task<Message> {
        if self.is_trash_view {
            return Task::none();
        }

        self.context_menu = None;
        self.select_path(path.clone());
        self.rename_input_history.reset();
        self.renaming = Some(path);
        focus_rename_input_command()
    }

    pub(super) fn begin_rename_selected(&mut self) -> Task<Message> {
        if !self.file_browser_content_shortcuts_enabled() {
            return Task::none();
        }
        let Some(path) = self.selected.clone() else {
            return Task::none();
        };
        self.begin_rename(path)
    }

    pub(super) fn focus_created_entry_for_rename(&mut self) -> Task<Message> {
        let Some(path) = self.pending_created_entry_rename.clone() else {
            return Task::none();
        };
        if self.entry_for_path(&path).is_none() {
            return Task::none();
        }

        self.pending_created_entry_rename = None;
        self.begin_rename(path)
    }

    pub(super) fn apply_rename_input_change(&mut self, value: String) -> Task<Message> {
        self.rename_input_history
            .apply_input_change(&mut self.rename_input, value);
        Task::none()
    }

    pub(super) fn undo_rename_input_change(&mut self) -> Task<Message> {
        self.rename_input_history.undo(&mut self.rename_input);
        Task::none()
    }

    pub(super) fn redo_rename_input_change(&mut self) -> Task<Message> {
        self.rename_input_history.redo(&mut self.rename_input);
        Task::none()
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
        self.clear_global_error();
        match self.operation_queue.enqueue(operation) {
            FileOperationEnqueueOutcome::Queued { task_id } => {
                if let Some(pending_history) = pending_history {
                    self.operation_history
                        .track_pending(task_id, pending_history);
                }
            }
            FileOperationEnqueueOutcome::QueuedWithStorageWarning { task_id, error } => {
                self.show_global_error(error);
                if let Some(pending_history) = pending_history {
                    self.operation_history
                        .track_pending(task_id, pending_history);
                }
            }
            FileOperationEnqueueOutcome::Rejected { error } => {
                self.show_global_error(error);
                if let Some(pending_history) = pending_history {
                    self.operation_history.reject_pending(pending_history);
                }
            }
        }
        Task::none()
    }
}

fn focus_rename_input_command() -> Task<Message> {
    let input_id = rename_input_id();
    Task::batch([
        iced::widget::operation::focus(input_id.clone()),
        iced::widget::operation::select_all(input_id),
    ])
}

#[cfg(test)]
mod tests;
