use iced::Task;
use std::path::PathBuf;

use super::{operation_queue_auto_hide_command, FileBrowser};
use crate::model::{Message, OperationQueuePanelMode};
use crate::operation_history::{
    path_after_completed_migrations, FileOperationCompletion, PendingHistoryOperation,
};
use crate::operation_queue::QueuedFileOperation;
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
        let completed_successfully = matches!(completion, FileOperationCompletion::Succeeded(_));
        let is_history_replay = self.operation_history.is_replaying(task_id);
        let created_path = (completed_successfully && !is_history_replay)
            .then(|| {
                self.operation_queue
                    .operation(task_id)
                    .and_then(QueuedFileOperation::created_path)
            })
            .flatten();

        if completed_successfully {
            if let Some(path) = created_path {
                self.pending_created_entry_rename = Some(path);
            }
        }

        let search_refresh_task = self.migrate_paths_after_file_operation(&completion);
        match &completion {
            FileOperationCompletion::Succeeded(outcome) => {
                self.operation_history.accept_completed(task_id, outcome);
            }
            FileOperationCompletion::Failed {
                completed_move_transfers,
                ..
            } => self
                .operation_history
                .accept_failed(task_id, completed_move_transfers),
        }

        let queue_result = match &completion {
            FileOperationCompletion::Succeeded(_) => Ok(()),
            FileOperationCompletion::Failed { error, .. } => Err(error.clone()),
        };
        let (finished, storage_error) = self.operation_queue.finish(task_id, queue_result);
        if let Some(error) = storage_error {
            self.show_global_error(error);
        }

        let pane_reload_task = if finished {
            if let Some(operation) = completed_operation.as_ref() {
                self.invalidate_list_directory_summaries_for_file_operation(operation);
                self.reload_visible_panes_after_file_operation_preserving_list_directory_summaries()
            } else {
                self.reload_visible_panes_after_file_operation()
            }
        } else {
            Task::none()
        };
        Task::batch([search_refresh_task, pane_reload_task])
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
        if let Some(error) = self.operation_queue.enqueue(operation) {
            self.show_global_error(error);
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

    use file_core::{DirectoryEntry, EntryMetadata, FileKind};
    use file_search::SearchScope;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::config;
    use crate::model::{
        BrowserPaneId, BrowserPaneLayout, BrowserTab, ExpandedDirectory, ExpandedDirectoryStatus,
        ListDirectorySummary, SplitAxis,
    };
    use crate::operation_history::FileOperationOutcome;
    use crate::thumbnail_cache::ColumnViewport;

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

    fn finish_queued_rename(browser: &mut FileBrowser, from: PathBuf, to: PathBuf) {
        assert!(browser
            .operation_queue
            .enqueue(QueuedFileOperation::Rename {
                path: from.clone(),
                new_name: to
                    .file_name()
                    .expect("rename target name")
                    .to_string_lossy()
                    .into_owned(),
            })
            .is_none());
        let task_id = browser
            .operation_queue
            .tasks()
            .last()
            .expect("queued rename")
            .id;
        drop(browser.accept_file_operation_finished(
            task_id,
            FileOperationCompletion::Succeeded(FileOperationOutcome::Rename { from, to }),
        ));
    }

    fn loaded_expanded_directory() -> ExpandedDirectory {
        ExpandedDirectory {
            entries: Vec::new(),
            status: ExpandedDirectoryStatus::Loaded,
            is_expanded: true,
            is_collapsing: false,
            animation_progress: 1.0,
            load_generation: 0,
            load_cancel: None,
        }
    }

    fn directory_entry(path: &str) -> DirectoryEntry {
        DirectoryEntry::new(
            PathBuf::from(path),
            FileKind::Directory,
            EntryMetadata::default(),
            false,
            false,
            false,
        )
    }

    #[test]
    fn rename_input_history_undoes_and_redoes_complete_snapshots() {
        let mut history = RenameInputHistory::default();
        let mut current_value = String::from("report.txt");

        history.apply_input_change(&mut current_value, String::from("report-1.txt"));
        history.apply_input_change(&mut current_value, String::from("report-2.txt"));
        history.undo(&mut current_value);
        assert_eq!(current_value, "report-1.txt");
        history.undo(&mut current_value);
        assert_eq!(current_value, "report.txt");
        history.redo(&mut current_value);
        assert_eq!(current_value, "report-1.txt");
        history.redo(&mut current_value);
        assert_eq!(current_value, "report-2.txt");
    }

    #[test]
    fn rename_input_history_clears_redo_branch_after_new_input() {
        let mut history = RenameInputHistory::default();
        let mut current_value = String::from("report.txt");

        history.apply_input_change(&mut current_value, String::from("report-1.txt"));
        history.undo(&mut current_value);
        history.apply_input_change(&mut current_value, String::from("report-final.txt"));
        history.redo(&mut current_value);

        assert_eq!(current_value, "report-final.txt");
    }

    #[test]
    fn beginning_new_rename_session_resets_input_history() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        drop(browser.begin_rename(PathBuf::from("/workspace/first.txt")));
        drop(browser.apply_rename_input_change(String::from("first-draft.txt")));
        drop(browser.begin_rename(PathBuf::from("/workspace/second.txt")));
        drop(browser.undo_rename_input_change());

        assert_eq!(browser.rename_input, "second.txt");

        drop(browser.apply_rename_input_change(String::from("second-draft.txt")));
        drop(browser.undo_rename_input_change());
        drop(browser.begin_rename(PathBuf::from("/workspace/third.txt")));
        drop(browser.redo_rename_input_change());

        assert_eq!(browser.rename_input, "third.txt");
    }

    #[test]
    fn completed_background_operation_preserves_active_rename_history() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let edited_path = PathBuf::from("/workspace/report.txt");

        drop(browser.begin_rename(edited_path.clone()));
        drop(browser.apply_rename_input_change(String::from("report-draft.txt")));
        assert!(browser
            .operation_queue
            .enqueue(QueuedFileOperation::DeletePermanently {
                paths: vec![PathBuf::from("/workspace/obsolete.txt")],
            })
            .is_none());
        let task_id = browser
            .operation_queue
            .tasks()
            .last()
            .expect("queued task")
            .id;

        drop(browser.accept_file_operation_finished(
            task_id,
            FileOperationCompletion::Succeeded(FileOperationOutcome::NoHistory),
        ));

        assert_eq!(browser.renaming, Some(edited_path));
        assert_eq!(browser.rename_input, "report-draft.txt");
        drop(browser.undo_rename_input_change());
        assert_eq!(browser.rename_input, "report.txt");
    }

    #[test]
    fn completed_path_migration_updates_inline_rename_and_restarts_search() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let source = PathBuf::from("/workspace/old");
        let destination = PathBuf::from("/workspace/new");
        browser.current_dir = source.join("nested");
        browser.renaming = Some(source.join("nested/report.txt"));
        browser.search.input = "report".to_owned();
        drop(browser.submit_search());
        let previous_search_generation = browser.search.generation;
        browser.sync_active_tab_state();

        finish_queued_rename(&mut browser, source, destination.clone());

        assert_eq!(
            browser.renaming,
            Some(destination.join("nested/report.txt"))
        );
        assert!(browser.search.generation > previous_search_generation);
        assert!(!browser
            .search
            .accepts_indexed_outcome(previous_search_generation));
        let active_query = browser
            .search
            .active_query
            .as_ref()
            .expect("restarted query");
        assert_eq!(
            active_query.scope,
            SearchScope::Directory(destination.join("nested"))
        );
    }

    #[test]
    fn cross_directory_move_invalidates_source_and_target_tab_caches() {
        let (browser, _) = FileBrowser::new(config::default_user_config());
        let source_directory = PathBuf::from("/source");
        let target_directory = PathBuf::from("/target");
        let source_path = source_directory.join("item");
        let target_path = target_directory.join("item");
        let source_load_cancellation = CancellationToken::new();
        let target_load_cancellation = CancellationToken::new();

        let mut pane = browser.capture_active_pane_snapshot();
        pane.current_dir = source_directory.clone();
        pane.entries = vec![directory_entry("/source/item")];
        pane.selected = Some(source_path.clone());
        pane.expanded_directories.insert(
            source_path.clone(),
            ExpandedDirectory {
                entries: Vec::new(),
                status: ExpandedDirectoryStatus::Loading,
                is_expanded: true,
                is_collapsing: false,
                animation_progress: 1.0,
                load_generation: 1,
                load_cancel: Some(source_load_cancellation.clone()),
            },
        );
        let source_tab_id = 20;
        let target_tab_id = 21;
        pane.tabs = vec![BrowserTab::directory(source_tab_id, source_directory), {
            let mut target_tab = BrowserTab::directory(target_tab_id, target_directory);
            target_tab.entries = vec![directory_entry("/target/existing")];
            target_tab.expanded_directories.insert(
                PathBuf::from("/target/existing"),
                ExpandedDirectory {
                    entries: Vec::new(),
                    status: ExpandedDirectoryStatus::Loading,
                    is_expanded: true,
                    is_collapsing: false,
                    animation_progress: 1.0,
                    load_generation: 1,
                    load_cancel: Some(target_load_cancellation.clone()),
                },
            );
            target_tab
        }];
        pane.active_tab_id = source_tab_id;
        pane.sync_active_tab_state();

        let outcome = FileOperationOutcome::Move {
            transfers: vec![crate::operation_history::CompletedTransfer {
                source: source_path,
                target: target_path,
            }],
            history_eligibility:
                crate::operation_history::FileOperationHistoryEligibility::Replayable,
        };
        pane.migrate_completed_paths(&outcome.completed_path_migrations());

        assert!(pane.entries.is_empty());
        assert!(pane.selected.is_none());
        assert!(pane.expanded_directories.is_empty());
        assert!(source_load_cancellation.is_cancelled());
        let target_tab = pane
            .tabs
            .iter()
            .find(|tab| tab.id == target_tab_id)
            .expect("target tab");
        assert!(target_tab.entries.is_empty());
        assert!(target_tab.expanded_directories.is_empty());
        assert!(target_load_cancellation.is_cancelled());
    }

    #[test]
    fn completed_rename_migrates_another_pane_hidden_tab_history_and_columns() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let source = PathBuf::from("/workspace/old");
        let destination = PathBuf::from("/workspace/new");
        browser.current_dir = PathBuf::from("/workspace/active");
        browser
            .column_return_targets
            .insert(source.join("column"), source.join("column/child"));
        browser.sync_active_tab_state();

        let active_tab_id = 10;
        let hidden_tab_id = 11;
        let mut inactive_pane = browser.capture_active_pane_snapshot();
        inactive_pane.id = BrowserPaneId(1);
        inactive_pane.current_dir = source.join("nested");
        inactive_pane.selected = Some(source.join("nested/file.txt"));
        inactive_pane
            .selected_paths
            .insert(source.join("nested/file.txt"));
        inactive_pane.deepest_open_column_directory = Some(source.join("nested"));
        inactive_pane
            .expanded_directories
            .insert(source.join("nested"), loaded_expanded_directory());
        inactive_pane.column_viewports.insert(
            source.join("nested"),
            ColumnViewport {
                offset_y: 24.0,
                height: 300.0,
            },
        );
        inactive_pane.back_stack = vec![source.join("back"), PathBuf::from("/workspace/old-copy")];
        inactive_pane.forward_stack = vec![source.join("forward")];
        inactive_pane.tabs = vec![
            BrowserTab::directory(active_tab_id, source.join("nested")),
            {
                let mut hidden_tab = BrowserTab::directory(hidden_tab_id, source.join("hidden"));
                hidden_tab
                    .expanded_directories
                    .insert(source.join("hidden/expanded"), loaded_expanded_directory());
                hidden_tab.back_stack.push(source.join("hidden/back"));
                hidden_tab
            },
        ];
        inactive_pane.active_tab_id = active_tab_id;
        inactive_pane.sync_active_tab_state();
        browser.panes.push(inactive_pane);
        browser.pane_layout = BrowserPaneLayout::Split {
            axis: SplitAxis::Horizontal,
            first: BrowserPaneId::PRIMARY,
            second: BrowserPaneId(1),
            active: BrowserPaneId::PRIMARY,
        };

        finish_queued_rename(&mut browser, source, destination.clone());

        assert_eq!(browser.current_dir, PathBuf::from("/workspace/active"));
        let migrated_pane = browser.pane_by_id(BrowserPaneId(1)).expect("inactive pane");
        assert_eq!(migrated_pane.current_dir, destination.join("nested"));
        assert_eq!(
            migrated_pane.back_stack,
            vec![
                destination.join("back"),
                PathBuf::from("/workspace/old-copy")
            ]
        );
        assert_eq!(
            migrated_pane.forward_stack,
            vec![destination.join("forward")]
        );
        assert!(migrated_pane
            .expanded_directories
            .contains_key(&destination.join("nested")));
        assert!(migrated_pane
            .column_viewports
            .contains_key(&destination.join("nested")));
        assert!(migrated_pane.directory_load_generation > 0);
        let hidden_tab = migrated_pane
            .tabs
            .iter()
            .find(|tab| tab.id == hidden_tab_id)
            .expect("hidden tab");
        assert_eq!(hidden_tab.directory, destination.join("hidden"));
        assert_eq!(hidden_tab.back_stack, vec![destination.join("hidden/back")]);
        assert!(hidden_tab
            .expanded_directories
            .contains_key(&destination.join("hidden/expanded")));
        assert_eq!(
            browser
                .column_return_targets
                .get(&destination.join("column")),
            Some(&destination.join("column/child"))
        );
    }

    #[test]
    fn undo_and_redo_reuse_success_completion_path_migration() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let source = PathBuf::from("/workspace/old");
        let destination = PathBuf::from("/workspace/new");
        browser.current_dir = source.join("nested");
        browser.sync_active_tab_state();

        finish_queued_rename(&mut browser, source.clone(), destination.clone());
        assert_eq!(browser.current_dir, destination.join("nested"));

        drop(browser.undo_file_operation());
        let undo_task_id = browser
            .operation_queue
            .tasks()
            .last()
            .expect("queued undo")
            .id;
        drop(browser.accept_file_operation_finished(
            undo_task_id,
            FileOperationCompletion::Succeeded(FileOperationOutcome::Rename {
                from: destination.clone(),
                to: source.clone(),
            }),
        ));
        assert_eq!(browser.current_dir, source.join("nested"));

        drop(browser.redo_file_operation());
        let redo_task_id = browser
            .operation_queue
            .tasks()
            .last()
            .expect("queued redo")
            .id;
        drop(browser.accept_file_operation_finished(
            redo_task_id,
            FileOperationCompletion::Succeeded(FileOperationOutcome::Rename {
                from: source,
                to: destination.clone(),
            }),
        ));
        assert_eq!(browser.current_dir, destination.join("nested"));
    }

    #[test]
    fn failed_move_migrates_paths_for_completed_transfers_only() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let source = PathBuf::from("/workspace/old");
        let destination = PathBuf::from("/archive/old");
        browser.current_dir = source.join("nested");
        browser.sync_active_tab_state();

        assert!(browser
            .operation_queue
            .enqueue(QueuedFileOperation::Move {
                transfers: Vec::new(),
                verification: file_core::FileOperationVerification::default(),
            })
            .is_none());
        let task_id = browser
            .operation_queue
            .tasks()
            .last()
            .expect("queued move")
            .id;

        drop(browser.accept_file_operation_finished(
            task_id,
            FileOperationCompletion::failed_after_completed_moves(
                "second transfer failed".to_owned(),
                vec![crate::operation_history::CompletedTransfer {
                    source,
                    target: destination.clone(),
                }],
            ),
        ));

        assert_eq!(browser.current_dir, destination.join("nested"));
        assert_eq!(
            browser
                .operation_queue
                .tasks()
                .last()
                .and_then(|task| task.error.as_deref()),
            Some("second transfer failed")
        );
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

        drop(browser.accept_file_operation_finished(
            task_id,
            FileOperationCompletion::Succeeded(FileOperationOutcome::NoHistory),
        ));

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

        drop(browser.accept_file_operation_finished(
            task_id,
            FileOperationCompletion::Succeeded(FileOperationOutcome::NoHistory),
        ));

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
