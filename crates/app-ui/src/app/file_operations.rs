use iced::widget::text_input;
use iced::Command;

use super::{operation_queue_auto_hide_command, FileBrowser};
use crate::model::Message;
use crate::operation_queue::QueuedFileOperation;
use crate::view::rename_input_id;

impl FileBrowser {
    pub(super) fn accept_file_operation_finished(
        &mut self,
        task_id: u64,
        result: Result<(), String>,
    ) -> Command<Message> {
        let completed_successfully = result.is_ok();
        let created_path = completed_successfully
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

        let (finished, storage_error) = self.operation_queue.finish(task_id, result);
        if let Some(error) = storage_error {
            self.error = Some(error);
        }

        if finished {
            self.reload_current()
        } else {
            Command::none()
        }
    }

    pub(super) fn commit_rename(&mut self) -> Command<Message> {
        let Some(path) = self.renaming.clone().or_else(|| self.selected.clone()) else {
            return Command::none();
        };

        let name = self.rename_input.trim();
        if name.is_empty() {
            self.renaming = None;
            return Command::none();
        }

        let old_name = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        if old_name == name {
            self.renaming = None;
            return Command::none();
        }

        self.renaming = None;
        self.context_menu = None;
        self.enqueue_file_operation(QueuedFileOperation::Rename {
            path,
            new_name: name.to_owned(),
        })
    }

    pub(super) fn commit_rename_if_active(&mut self) -> Command<Message> {
        if self.renaming.is_some() {
            self.commit_rename()
        } else {
            Command::none()
        }
    }

    pub(super) fn focus_created_entry_for_rename(&mut self) -> Command<Message> {
        let Some(path) = self.pending_created_entry_rename.clone() else {
            return Command::none();
        };
        if self.entry_for_path(&path).is_none() {
            return Command::none();
        }

        self.pending_created_entry_rename = None;
        self.select_path(path.clone());
        self.renaming = Some(path);
        let input_id = rename_input_id();
        Command::batch([
            text_input::focus(input_id.clone()),
            text_input::select_all(input_id),
        ])
    }

    pub(super) fn enqueue_file_operation(
        &mut self,
        operation: QueuedFileOperation,
    ) -> Command<Message> {
        self.error = None;
        if let Some(error) = self.operation_queue.enqueue(operation) {
            self.error = Some(error);
        }
        self.show_operation_queue_temporarily()
    }

    fn show_operation_queue_temporarily(&mut self) -> Command<Message> {
        self.operation_queue.open_panel();
        self.operation_queue_auto_hide_generation =
            self.operation_queue_auto_hide_generation.wrapping_add(1);
        operation_queue_auto_hide_command(self.operation_queue_auto_hide_generation)
    }
}
