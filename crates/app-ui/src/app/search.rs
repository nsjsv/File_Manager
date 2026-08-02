use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::Local;
use file_search::{SearchExcludeRules, SearchHit, SearchQuery, SearchScope};
use iced::Task;

use super::{FileBrowser, DOUBLE_CLICK_THRESHOLD};
use crate::commands::{
    directory_fallback_search_command, open_file_command, search_command,
    search_service_recovery_command, search_service_status_command,
};
use crate::model::search::SEARCH_RESULT_WINDOW;
use crate::model::{
    ContextMenuState, DestructiveActionConfirmation, DirectoryFallbackOutcome,
    IndexedSearchOutcome, LastActivationClick, Message, ModifiedTimePreset, NavigationMode,
    SearchContentCategory, SearchContextMenuState, SearchKeyboardSelection, SearchObjectType,
    SearchSelectionGesture, SearchSelectionStep, SearchServiceDiagnostic,
    SearchServiceDiagnosticKind, SearchServiceRecoveryAction, SearchServiceStatusRequest,
    SearchWorkspaceState,
};
use crate::shortcuts::{FileSelectionDirection, ShortcutAction};

impl FileBrowser {
    pub(crate) fn invoke_search_workspace_shortcut(
        &mut self,
        action: ShortcutAction,
    ) -> Task<Message> {
        match action {
            ShortcutAction::OpenSelected => self.activate_selected_search_result(),
            ShortcutAction::MoveSelection(direction) => {
                self.move_search_result_selection(direction)
            }
            ShortcutAction::SelectAll => self.select_all_search_results(),
            ShortcutAction::Copy => self.copy_selected(),
            ShortcutAction::Cut => self.move_selected(),
            ShortcutAction::Delete => self.trash_selected(),
            ShortcutAction::Refresh => self.submit_search(),
            ShortcutAction::Undo => self.undo_file_operation(),
            ShortcutAction::Redo => self.redo_file_operation(),
            ShortcutAction::Escape if !self.file_browser_content_shortcuts_enabled() => {
                self.handle_focused_window_escape_pressed()
            }
            ShortcutAction::Escape => self.close_search_workspace(),
            ShortcutAction::RenameSelected
            | ShortcutAction::FocusPathInput
            | ShortcutAction::NavigateBack
            | ShortcutAction::NavigateForward
            | ShortcutAction::NavigateUp
            | ShortcutAction::FileProperties
            | ShortcutAction::Preview
            | ShortcutAction::Paste => Task::none(),
        }
    }

    pub(super) fn update_search_input(&mut self, value: String) -> Task<Message> {
        if let Err(message) = self.ensure_search_workspace() {
            self.show_global_error(message);
            return Task::none();
        }
        let Some(workspace) = self.search_workspace.as_mut() else {
            return Task::none();
        };
        workspace.input = value;
        self.restart_search_workspace()
    }

    pub(super) fn submit_search(&mut self) -> Task<Message> {
        if let Err(message) = self.ensure_search_workspace() {
            self.show_global_error(message);
            return Task::none();
        }
        self.restart_search_workspace()
    }

    pub(super) fn clear_search_keyword(&mut self) -> Task<Message> {
        let Some(workspace) = self.search_workspace.as_mut() else {
            return Task::none();
        };
        workspace.input.clear();
        self.restart_search_workspace()
    }

    pub(super) fn select_search_object_type(
        &mut self,
        object_type: SearchObjectType,
    ) -> Task<Message> {
        let Some(workspace) = self.search_workspace.as_mut() else {
            return Task::none();
        };
        workspace.filters.select_object_type(object_type);
        self.restart_search_workspace()
    }

    pub(super) fn select_search_content_category(
        &mut self,
        content_category: SearchContentCategory,
    ) -> Task<Message> {
        let Some(workspace) = self.search_workspace.as_mut() else {
            return Task::none();
        };
        workspace.filters.select_content_category(content_category);
        self.restart_search_workspace()
    }

    pub(super) fn select_search_modified_time(
        &mut self,
        modified_time: ModifiedTimePreset,
    ) -> Task<Message> {
        let Some(workspace) = self.search_workspace.as_mut() else {
            return Task::none();
        };
        workspace.filters.modified_time = modified_time;
        self.restart_search_workspace()
    }

    pub(super) fn close_search_workspace(&mut self) -> Task<Message> {
        self.discard_search_workspace();
        Task::none()
    }

    pub(super) fn discard_search_workspace(&mut self) {
        self.search_workspace = None;
        self.last_activation_click = None;
        if matches!(self.context_menu, Some(ContextMenuState::Search(_))) {
            self.context_menu = None;
        }
    }

    fn ensure_search_workspace(&mut self) -> Result<(), String> {
        if self.search_workspace.is_some() {
            return Ok(());
        }
        if self.is_trash_view {
            return Err("Search is available from a local folder".to_owned());
        }
        let root = self.current_search_directory();
        if self.path_is_remote_mount(&root) {
            return Err("Search is unavailable for remote folders".to_owned());
        }
        self.search_workspace = Some(SearchWorkspaceState::new(root));
        Ok(())
    }

    fn restart_search_workspace(&mut self) -> Task<Message> {
        let Some(workspace) = self.search_workspace.as_ref() else {
            return Task::none();
        };
        let generation = workspace.next_generation();
        let root = workspace.root.path().to_path_buf();
        if !std::fs::metadata(&root).is_ok_and(|metadata| metadata.is_dir()) {
            if let Some(workspace) = self.search_workspace.as_mut() {
                workspace.reject_query(format!("Search root is unavailable: {}", root.display()));
            }
            return Task::none();
        }
        let filters = match workspace.filters.query_filters_at(Local::now()) {
            Ok(filters) => filters,
            Err(message) => {
                if let Some(workspace) = self.search_workspace.as_mut() {
                    workspace.reject_query(message);
                }
                return Task::none();
            }
        };
        let query = SearchQuery {
            query_id: generation,
            terms: workspace.input.trim().to_owned(),
            scope: SearchScope::Directory(root),
            recursive: true,
            filters,
            limit: SEARCH_RESULT_WINDOW,
            cursor: None,
        };
        let Some(workspace) = self.search_workspace.as_mut() else {
            return Task::none();
        };
        let cancellation = workspace.begin_indexed_query(query.clone());
        search_command(generation, query, cancellation)
    }

    pub(super) fn accept_search_results(
        &mut self,
        generation: u64,
        outcome: IndexedSearchOutcome,
    ) -> Task<Message> {
        if !self
            .search_workspace
            .as_ref()
            .is_some_and(|workspace| workspace.accepts_indexed_outcome(generation))
        {
            return Task::none();
        }
        match outcome {
            IndexedSearchOutcome::Cancelled => {
                if let Some(workspace) = self.search_workspace.as_mut() {
                    workspace.apply_indexed_cancellation();
                }
            }
            IndexedSearchOutcome::Batch(batch) => {
                if let Some(workspace) = self.search_workspace.as_mut() {
                    workspace.apply_indexed_batch(batch);
                }
            }
            IndexedSearchOutcome::TransportUnavailable(message) => {
                self.search_service
                    .observe_query_transport_failure(&message);
                return self.switch_to_directory_fallback(message);
            }
            IndexedSearchOutcome::ProviderUnavailable(message) => {
                return self.switch_to_directory_fallback(message);
            }
            IndexedSearchOutcome::InvalidQuery(message) | IndexedSearchOutcome::Fatal(message) => {
                if let Some(workspace) = self.search_workspace.as_mut() {
                    workspace.apply_indexed_failure(message);
                }
            }
        }
        Task::none()
    }

    pub(super) fn accept_directory_search_batch(
        &mut self,
        generation: u64,
        hits: Vec<SearchHit>,
    ) -> Task<Message> {
        if let Some(workspace) = self
            .search_workspace
            .as_mut()
            .filter(|workspace| workspace.accepts_directory_fallback(generation))
        {
            workspace.apply_directory_batch(hits);
        }
        Task::none()
    }

    pub(super) fn accept_directory_search_finished(
        &mut self,
        generation: u64,
        completion: DirectoryFallbackOutcome,
    ) -> Task<Message> {
        if let Some(workspace) = self
            .search_workspace
            .as_mut()
            .filter(|workspace| workspace.accepts_directory_fallback(generation))
        {
            workspace.finish_directory_fallback(completion);
        }
        Task::none()
    }

    pub(super) fn press_search_result(&mut self, path: PathBuf) -> Task<Message> {
        let has_selection_modifier =
            self.keyboard_modifiers.control() || self.keyboard_modifiers.shift();
        let now = Instant::now();
        let is_double_click = !has_selection_modifier
            && self
                .last_activation_click
                .as_ref()
                .is_some_and(|last_click| {
                    last_click.path == path
                        && now.duration_since(last_click.at) <= DOUBLE_CLICK_THRESHOLD
                });
        let gesture = match (
            self.keyboard_modifiers.control(),
            self.keyboard_modifiers.shift(),
        ) {
            (true, true) => SearchSelectionGesture::AdditiveRange,
            (false, true) => SearchSelectionGesture::Range,
            (true, false) => SearchSelectionGesture::Toggle,
            (false, false) => SearchSelectionGesture::Plain,
        };
        let Some(workspace) = self.search_workspace.as_mut() else {
            return Task::none();
        };
        workspace
            .selection
            .select(&workspace.window.hits, &path, gesture);
        self.context_menu = None;
        self.last_activation_click = if has_selection_modifier {
            None
        } else {
            Some(LastActivationClick {
                path: path.clone(),
                at: now,
            })
        };

        if is_double_click {
            self.activate_search_path(path)
        } else {
            Task::none()
        }
    }

    pub(super) fn right_click_search_result(&mut self, path: PathBuf) -> Task<Message> {
        let Some(workspace) = self.search_workspace.as_mut() else {
            return Task::none();
        };
        if !workspace.selection.is_selected(&path) {
            workspace.selection.select(
                &workspace.window.hits,
                &path,
                SearchSelectionGesture::Plain,
            );
        }
        self.context_menu = Some(ContextMenuState::Search(SearchContextMenuState {
            target: path,
            position: self.cursor_position,
        }));
        Task::none()
    }

    pub(crate) fn activate_selected_search_result(&mut self) -> Task<Message> {
        let Some(path) = self
            .search_workspace
            .as_ref()
            .and_then(|workspace| workspace.selection.focused_path())
            .map(Path::to_path_buf)
        else {
            return Task::none();
        };
        self.activate_search_path(path)
    }

    pub(crate) fn move_search_result_selection(
        &mut self,
        direction: FileSelectionDirection,
    ) -> Task<Message> {
        let step = match direction {
            FileSelectionDirection::Up => SearchSelectionStep::Previous,
            FileSelectionDirection::Down => SearchSelectionStep::Next,
            FileSelectionDirection::Left | FileSelectionDirection::Right => return Task::none(),
        };
        let keyboard_selection = if self.keyboard_modifiers.shift() {
            SearchKeyboardSelection::Extend
        } else {
            SearchKeyboardSelection::Replace
        };
        let Some(workspace) = self.search_workspace.as_mut() else {
            return Task::none();
        };
        let Some(target) =
            workspace
                .selection
                .move_focus(&workspace.window.hits, step, keyboard_selection)
        else {
            return Task::none();
        };
        let Some(index) = workspace
            .window
            .hits
            .iter()
            .position(|hit| hit.path == target)
        else {
            return Task::none();
        };
        Task::batch([
            iced::widget::operation::scroll_to(
                crate::app::smooth_scroll::smooth_scroll_id(
                    &crate::model::ScrollbarRegion::SearchResults,
                ),
                iced::widget::scrollable::AbsoluteOffset {
                    x: 0.0,
                    y: index as f32 * crate::view::SEARCH_RESULT_ROW_HEIGHT,
                },
            ),
            self.show_scrollbars_temporarily(crate::model::ScrollbarRegion::SearchResults),
        ])
    }

    pub(crate) fn select_all_search_results(&mut self) -> Task<Message> {
        let Some(workspace) = self.search_workspace.as_mut() else {
            return Task::none();
        };
        workspace.selection.select_all(&workspace.window.hits);
        Task::none()
    }

    pub(crate) fn active_search_selection(&self) -> Option<Vec<PathBuf>> {
        self.search_workspace
            .as_ref()
            .map(SearchWorkspaceState::selected_paths_in_result_order)
    }

    pub(super) fn delete_search_selection_permanently(&mut self) -> Task<Message> {
        self.context_menu = None;
        let Some(paths) = self.active_search_selection() else {
            return Task::none();
        };
        if paths.is_empty() {
            return Task::none();
        }
        self.request_destructive_action_confirmation(
            DestructiveActionConfirmation::DeletePermanently { paths },
        );
        Task::none()
    }

    pub(super) fn open_search_containing_directory(&mut self, path: PathBuf) -> Task<Message> {
        let Some(parent) = path.parent().map(Path::to_path_buf) else {
            self.fail_search_workspace("The result has no containing directory".to_owned());
            return Task::none();
        };
        if !parent.is_dir() {
            self.fail_search_workspace(format!(
                "Containing directory is unavailable: {}",
                parent.display()
            ));
            return Task::none();
        }

        self.discard_search_workspace();
        let navigation = self.navigate_to(parent, NavigationMode::RecordHistory);
        self.select_path(path);
        Task::batch([navigation, self.request_browser_session_save()])
    }

    fn activate_search_path(&mut self, path: PathBuf) -> Task<Message> {
        let Some(hit) = self
            .search_workspace
            .as_ref()
            .and_then(|workspace| workspace.hit_for_path(&path))
            .cloned()
        else {
            return Task::none();
        };
        if hit.kind == file_search::SearchFileKind::Directory {
            self.discard_search_workspace();
            self.navigate_to(hit.path, NavigationMode::RecordHistory)
        } else {
            open_file_command(hit.path, self.terminal_emulator)
        }
    }

    fn fail_search_workspace(&mut self, message: String) {
        if let Some(workspace) = self.search_workspace.as_mut() {
            workspace.window.failure = Some(message);
            workspace.window.is_loading = false;
        }
    }

    pub(super) fn restart_search_service(&mut self) -> Task<Message> {
        self.search_service
            .begin_restart()
            .map(search_service_recovery_command)
            .unwrap_or_else(Task::none)
    }

    pub(super) fn press_force_restart_search_service(&mut self) -> Task<Message> {
        self.search_service
            .press_force_restart()
            .map(search_service_recovery_command)
            .unwrap_or_else(Task::none)
    }

    pub(super) fn accept_search_service_recovery(
        &mut self,
        action: SearchServiceRecoveryAction,
        outcome: Result<file_search::SearchServiceStatus, SearchServiceDiagnostic>,
    ) -> Task<Message> {
        self.search_service
            .accept_recovery_completion(action, outcome);
        Task::none()
    }

    pub(super) fn refresh_search_service_status(&mut self) -> Task<Message> {
        self.search_service
            .request_status_refresh()
            .map(search_service_status_command)
            .unwrap_or_else(Task::none)
    }

    pub(super) fn accept_search_service_status(
        &mut self,
        request: SearchServiceStatusRequest,
        outcome: Result<file_search::SearchServiceStatus, SearchServiceDiagnostic>,
    ) -> Task<Message> {
        self.search_service.accept_status_request(request, outcome);
        Task::none()
    }

    pub(super) fn toggle_search_service_incident_details(
        &mut self,
        kind: SearchServiceDiagnosticKind,
    ) -> Task<Message> {
        self.search_service.toggle_incident_technical_detail(kind);
        Task::none()
    }

    pub(super) fn copy_search_service_incident_details(
        &self,
        kind: SearchServiceDiagnosticKind,
    ) -> Task<Message> {
        self.search_service
            .incident_technical_detail(kind)
            .map(iced::clipboard::write)
            .unwrap_or_else(Task::none)
    }

    pub(super) fn toggle_search_content_indexing(&mut self) -> Task<Message> {
        self.user_config.search_content_indexing_enabled =
            !self.user_config.search_content_indexing_enabled;
        self.persist_app_config_command()
    }

    fn switch_to_directory_fallback(&mut self, unavailable_message: String) -> Task<Message> {
        let Some(workspace) = self.search_workspace.as_ref() else {
            return Task::none();
        };
        if workspace.run.indexed_batch_seen {
            if let Some(workspace) = self.search_workspace.as_mut() {
                workspace.apply_indexed_failure(unavailable_message);
            }
            return Task::none();
        }
        let Some(query) = workspace.run.active_query.clone() else {
            self.fail_search_workspace(unavailable_message);
            return Task::none();
        };
        if !query.recursive {
            self.fail_search_workspace(unavailable_message);
            return Task::none();
        }
        let SearchScope::Directory(directory) = &query.scope else {
            self.fail_search_workspace(unavailable_message);
            return Task::none();
        };
        if query.cursor.is_some() || self.path_is_remote_mount(directory) {
            self.fail_search_workspace(unavailable_message);
            return Task::none();
        }

        let generation = workspace.run.generation;
        let Some(workspace) = self.search_workspace.as_mut() else {
            return Task::none();
        };
        let cancellation = workspace.begin_directory_fallback();
        directory_fallback_search_command(
            generation,
            query,
            SearchExcludeRules::new(Vec::new()),
            cancellation,
        )
    }

    fn current_search_directory(&self) -> PathBuf {
        self.pane_by_id(self.active_pane_id())
            .map(|pane| pane.current_dir.clone())
            .unwrap_or_else(|| self.current_dir.clone())
    }
}

#[cfg(test)]
#[path = "search_operations_tests.rs"]
mod operation_tests;

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
