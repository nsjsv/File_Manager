use iced::Task;
use std::path::PathBuf;

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
        let completed_operation = self.operation_queue.operation(task_id).cloned();
        let completed_successfully = result.is_ok();
        let search_index_root = self
            .operation_queue
            .operation(task_id)
            .and_then(|operation| {
                if let QueuedFileOperation::BuildSearchIndex { root, .. } = operation {
                    Some(root.clone())
                } else {
                    None
                }
            });
        let search_index_result = match &result {
            Ok(FileOperationOutcome::SearchIndex { outcome }) => Some(Ok(outcome.clone())),
            Err(error) if search_index_root.is_some() => Some(Err(error.clone())),
            _ => None,
        };
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

        if let (Some(root), Some(index_result)) = (search_index_root, search_index_result) {
            return self.accept_search_index(root, index_result);
        }

        if finished {
            if let Some(operation) = completed_operation.as_ref() {
                self.invalidate_list_directory_summaries_for_file_operation(operation);
                self.reload_visible_panes_preserving_list_directory_summaries()
            } else {
                self.reload_visible_panes()
            }
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
        self.select_path(path.clone());
        self.renaming = Some(path);
        focus_rename_input_command()
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

    pub(super) fn show_operation_queue_temporarily(&mut self) -> Task<Message> {
        self.operation_queue_panel_mode = OperationQueuePanelMode::PassivePreview;
        self.operation_queue.open_panel();
        self.operation_queue_auto_hide_generation =
            self.operation_queue_auto_hide_generation.wrapping_add(1);
        operation_queue_auto_hide_command(self.operation_queue_auto_hide_generation)
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
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config;
    use crate::model::ListDirectorySummary;

    fn remember_summary(
        browser: &mut FileBrowser,
        path: &std::path::Path,
        count: usize,
        size: u64,
    ) {
        browser
            .list_directory_summary_cache
            .remember_direct_child_count(path.to_path_buf(), count);
        let request = browser
            .list_directory_summary_cache
            .start_request(path.to_path_buf(), true)
            .expect("recursive request");
        assert!(browser.list_directory_summary_cache.store_summary(
            &request,
            ListDirectorySummary {
                direct_child_count: count,
                recursive_total_size_bytes: Some(size),
            }
        ));
    }

    #[test]
    fn finished_delete_operation_only_invalidates_affected_directory_chain() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let root = PathBuf::from("/workspace");
        let current_dir = root.join("project");
        let deleted_child = current_dir.join("todo.txt");
        let unrelated = root.join("archive");

        browser.current_dir = current_dir.clone();
        browser.is_loading = false;

        remember_summary(&mut browser, &root, 3, 4096);
        remember_summary(&mut browser, &current_dir, 2, 2048);
        remember_summary(&mut browser, &unrelated, 1, 512);

        assert!(browser
            .operation_queue
            .enqueue(QueuedFileOperation::DeletePermanently {
                paths: vec![deleted_child],
            })
            .is_none());
        let task_id = browser
            .operation_queue
            .tasks()
            .last()
            .expect("queued task")
            .id;

        drop(browser.accept_file_operation_finished(task_id, Ok(FileOperationOutcome::NoHistory)));

        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&current_dir)
            .is_none());
        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&root)
            .is_none());
        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&unrelated)
            .is_some());
    }

    #[test]
    fn finished_directory_delete_operation_clears_cached_descendant_summaries() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let root = PathBuf::from("/workspace");
        let current_dir = root.join("project");
        let deleted_directory = current_dir.join("src");
        let deleted_descendant = deleted_directory.join("nested");
        let unrelated = root.join("archive");

        browser.current_dir = current_dir.clone();
        browser.is_loading = false;

        remember_summary(&mut browser, &root, 3, 4096);
        remember_summary(&mut browser, &current_dir, 2, 2048);
        remember_summary(&mut browser, &deleted_directory, 4, 1536);
        remember_summary(&mut browser, &deleted_descendant, 1, 256);
        remember_summary(&mut browser, &unrelated, 1, 512);

        assert!(browser
            .operation_queue
            .enqueue(QueuedFileOperation::DeletePermanently {
                paths: vec![deleted_directory],
            })
            .is_none());
        let task_id = browser
            .operation_queue
            .tasks()
            .last()
            .expect("queued task")
            .id;

        drop(browser.accept_file_operation_finished(task_id, Ok(FileOperationOutcome::NoHistory)));

        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&current_dir)
            .is_none());
        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&root)
            .is_none());
        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&deleted_descendant)
            .is_none());
        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&unrelated)
            .is_some());
    }
}
