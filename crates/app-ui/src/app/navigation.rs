use std::path::{Path, PathBuf};
use std::time::Duration;

use file_core::{DirectoryEntry, DirectoryScan, FileKind, TrashScan};
use iced::Task;

use super::paths::{completed_path_text, path_text};
use super::FileBrowser;
use crate::commands::{
    delayed_thumbnail_refresh_command, load_directory_command, load_expanded_directory_command,
    load_trash_command, path_suggestions_command,
};
use crate::model::{
    trash_location_path, ExpandedDirectory, ExpandedDirectoryStatus, Message, NavigationMode,
    PathSuggestionDirection, PathSuggestionRequest, TRASH_LOCATION_LABEL,
};
use crate::startup_trace;
use crate::view::path_input_id;

const PATH_SUGGESTION_INPUT_STABILIZATION_DELAY: Duration = Duration::from_millis(120);

impl FileBrowser {
    pub(super) fn accept_directory_scan(
        &mut self,
        pane_id: crate::model::BrowserPaneId,
        scan: DirectoryScan,
    ) -> Task<Message> {
        if pane_id != self.active_pane_id() {
            let Some(pane) = self.pane_by_id_mut(pane_id) else {
                return Task::none();
            };
            if scan.path != pane.current_dir {
                return Task::none();
            }

            pane.current_dir = scan.path;
            pane.path_input = path_text(&pane.current_dir);
            pane.path_suggestions.clear();
            pane.path_suggestion_selection = None;
            pane.entries = scan.entries;
            pane.is_loading = false;
            pane.sync_active_tab_state();
            return delayed_thumbnail_refresh_command(pane_id, pane.current_dir.clone());
        }

        if scan.path != self.current_dir {
            return Task::none();
        }

        self.current_dir = scan.path;
        self.path_input = path_text(&self.current_dir);
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        self.entries = scan.entries;
        self.reveal_pending_search_match();
        self.is_loading = false;
        self.error = None;
        startup_trace::mark_once("initial_directory_ready");
        let command = self.focus_created_entry_for_rename();
        self.sync_active_tab_state();
        Task::batch([
            command,
            delayed_thumbnail_refresh_command(pane_id, self.current_dir.clone()),
        ])
    }

    pub(super) fn accept_trash_scan(
        &mut self,
        pane_id: crate::model::BrowserPaneId,
        scan: TrashScan,
    ) -> Task<Message> {
        if pane_id != self.active_pane_id() {
            let Some(pane) = self.pane_by_id_mut(pane_id) else {
                return Task::none();
            };
            if !pane.is_trash_view {
                return Task::none();
            }

            pane.current_dir = trash_location_path();
            pane.path_input = TRASH_LOCATION_LABEL.to_owned();
            pane.path_suggestions.clear();
            pane.path_suggestion_selection = None;
            pane.trash_entries = scan.entries;
            pane.entries = pane
                .trash_entries
                .iter()
                .map(|trash_entry| trash_entry.entry.clone())
                .collect();
            pane.expanded_directories.clear();
            pane.is_loading = false;
            pane.sync_active_tab_state();
            return delayed_thumbnail_refresh_command(pane_id, pane.current_dir.clone());
        }

        if !self.is_trash_view {
            return Task::none();
        }

        self.current_dir = trash_location_path();
        self.path_input = TRASH_LOCATION_LABEL.to_owned();
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        self.trash_entries = scan.entries;
        self.entries = self
            .trash_entries
            .iter()
            .map(|trash_entry| trash_entry.entry.clone())
            .collect();
        self.expanded_directories.clear();
        self.is_loading = false;
        self.error = None;
        self.sync_active_tab_state();
        delayed_thumbnail_refresh_command(pane_id, self.current_dir.clone())
    }

    pub(super) fn navigate_to(&mut self, path: PathBuf, mode: NavigationMode) -> Task<Message> {
        let pane_id = self.active_pane_id();
        if mode == NavigationMode::RecordHistory && !self.is_trash_view && path != self.current_dir
        {
            self.back_stack.push(self.current_dir.clone());
            self.forward_stack.clear();
        }
        self.current_dir = path.clone();
        self.is_trash_view = false;
        self.path_input = path_text(&path);
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        self.entries.clear();
        self.trash_entries.clear();
        self.expanded_directories.clear();
        self.column_viewports.clear();
        self.clear_selection_context();
        self.is_loading = true;
        self.error = None;
        self.sync_active_tab_state();
        load_directory_command(pane_id, path, self.options.clone())
    }

    pub(super) fn open_trash_view(&mut self, mode: NavigationMode) -> Task<Message> {
        let pane_id = self.active_pane_id();
        if mode == NavigationMode::RecordHistory && !self.is_trash_view {
            self.back_stack.push(self.current_dir.clone());
            self.forward_stack.clear();
        }
        self.current_dir = trash_location_path();
        self.is_trash_view = true;
        self.path_input = TRASH_LOCATION_LABEL.to_owned();
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        self.entries.clear();
        self.trash_entries.clear();
        self.expanded_directories.clear();
        self.column_viewports.clear();
        self.clear_selection_context();
        self.is_loading = true;
        self.error = None;
        self.sync_active_tab_state();
        load_trash_command(pane_id, self.options.clone())
    }

    pub(super) fn reload_current(&mut self) -> Task<Message> {
        if self.is_trash_view {
            self.path_input = TRASH_LOCATION_LABEL.to_owned();
            self.path_suggestions.clear();
            self.path_suggestion_selection = None;
            self.clear_transient_interaction_state();
            self.is_loading = true;
            self.error = None;
            self.expanded_directories.clear();
            return load_trash_command(self.active_pane_id(), self.options.clone());
        }

        self.path_input = path_text(&self.current_dir);
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        self.clear_transient_interaction_state();
        self.is_loading = true;
        self.error = None;

        let mut commands = self.refresh_expanded_directory_commands();
        commands.push(load_directory_command(
            self.active_pane_id(),
            self.current_dir.clone(),
            self.options.clone(),
        ));
        Task::batch(commands)
    }

    pub(super) fn reload_observed_directory(&mut self, path: PathBuf) -> Task<Message> {
        if self.is_trash_view {
            return Task::none();
        }

        if path == self.current_dir {
            return self.reload_current();
        }

        let Some(expanded) = self.expanded_directories.get_mut(&path) else {
            return Task::none();
        };

        expanded.status = ExpandedDirectoryStatus::Loading;
        expanded.is_expanded = true;
        expanded.animation_progress = 1.0;
        load_expanded_directory_command(self.active_pane_id(), path, self.options.clone())
    }

    pub(super) fn navigate_up(&mut self) -> Task<Message> {
        if self.is_trash_view {
            return Task::none();
        }

        if let Some(parent) = self.current_dir.parent() {
            self.navigate_to(parent.to_path_buf(), NavigationMode::RecordHistory)
        } else {
            Task::none()
        }
    }

    pub(super) fn navigate_back(&mut self) -> Task<Message> {
        if self.is_trash_view {
            if let Some(path) = self.back_stack.pop() {
                return self.navigate_to(path, NavigationMode::KeepHistory);
            }
            return Task::none();
        }

        if let Some(path) = self.back_stack.pop() {
            self.forward_stack.push(self.current_dir.clone());
            self.navigate_to(path, NavigationMode::KeepHistory)
        } else {
            Task::none()
        }
    }

    pub(super) fn navigate_forward(&mut self) -> Task<Message> {
        if let Some(path) = self.forward_stack.pop() {
            self.back_stack.push(self.current_dir.clone());
            self.navigate_to(path, NavigationMode::KeepHistory)
        } else {
            Task::none()
        }
    }

    pub(super) fn update_path_input(&mut self, value: String) -> Task<Message> {
        self.path_input = value;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        let request = self.next_path_suggestion_request();
        if request.input.trim().is_empty() {
            return Task::none();
        }
        path_suggestion_input_stabilization_command(self.active_pane_id(), request)
    }

    pub(super) fn load_stable_path_suggestions(
        &mut self,
        pane_id: crate::model::BrowserPaneId,
        request: PathSuggestionRequest,
    ) -> Task<Message> {
        if pane_id != self.active_pane_id() {
            let Some(pane) = self.pane_by_id(pane_id) else {
                return Task::none();
            };
            if !path_suggestion_request_matches_state(
                &request,
                &pane.path_input,
                &pane.current_dir,
                pane.path_suggestion_generation,
            ) {
                return Task::none();
            }
            return path_suggestions_command(pane_id, request);
        }

        if !self.active_path_suggestion_request_matches(&request) {
            return Task::none();
        }
        path_suggestions_command(pane_id, request)
    }

    pub(super) fn accept_path_suggestions(
        &mut self,
        pane_id: crate::model::BrowserPaneId,
        request: PathSuggestionRequest,
        suggestions: Vec<PathBuf>,
    ) -> Task<Message> {
        if pane_id != self.active_pane_id() {
            let Some(pane) = self.pane_by_id_mut(pane_id) else {
                return Task::none();
            };
            if path_suggestion_request_matches_state(
                &request,
                &pane.path_input,
                &pane.current_dir,
                pane.path_suggestion_generation,
            ) {
                pane.path_suggestions = suggestions;
                if pane.path_suggestions.is_empty() {
                    pane.path_suggestion_selection = None;
                } else {
                    pane.path_suggestion_selection = pane
                        .path_suggestion_selection
                        .filter(|index| *index < pane.path_suggestions.len());
                }
            }
            return Task::none();
        }

        if self.active_path_suggestion_request_matches(&request) {
            self.path_suggestions = suggestions;
            self.normalize_path_suggestion_selection();
        }
        Task::none()
    }

    pub(super) fn move_search_or_path_suggestion_selection(
        &mut self,
        direction: PathSuggestionDirection,
    ) -> Task<Message> {
        if self.search.is_some() {
            return self.move_search_selection(direction);
        }
        self.move_path_suggestion_selection(direction);
        Task::none()
    }

    pub(super) fn complete_search_scope_or_path_suggestion(
        &mut self,
        direction: PathSuggestionDirection,
    ) -> Task<Message> {
        if self.search.is_some() {
            self.toggle_search_scope()
        } else {
            self.complete_path_suggestion(direction)
        }
    }

    pub(super) fn submit_path_input(&mut self) -> Task<Message> {
        let Some(typed_path) = self.path_from_input() else {
            self.path_input = path_text(&self.current_dir);
            self.path_suggestions.clear();
            self.path_suggestion_selection = None;
            return Task::none();
        };

        let selected_suggestion = self
            .path_suggestion_selection
            .and_then(|index| self.path_suggestions.get(index))
            .cloned();

        let path = selected_suggestion.unwrap_or(typed_path);

        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        Task::batch([
            self.navigate_to(path, NavigationMode::RecordHistory),
            iced::widget::operation::move_cursor_to_end(path_input_id(self.active_pane_id())),
        ])
    }

    pub(super) fn normalize_path_suggestion_selection(&mut self) {
        if self.path_suggestions.is_empty() {
            self.path_suggestion_selection = None;
            return;
        }

        self.path_suggestion_selection = self
            .path_suggestion_selection
            .filter(|index| *index < self.path_suggestions.len());
    }

    pub(super) fn move_path_suggestion_selection(&mut self, direction: PathSuggestionDirection) {
        if self.path_suggestions.is_empty() {
            self.path_suggestion_selection = None;
            return;
        }

        let last_index = self.path_suggestions.len() - 1;
        let Some(current) = self.path_suggestion_selection else {
            self.path_suggestion_selection = Some(match direction {
                PathSuggestionDirection::Next => 0,
                PathSuggestionDirection::Previous => last_index,
            });
            return;
        };
        self.path_suggestion_selection = Some(match direction {
            PathSuggestionDirection::Next => {
                if current >= last_index {
                    0
                } else {
                    current + 1
                }
            }
            PathSuggestionDirection::Previous => {
                if current == 0 {
                    last_index
                } else {
                    current - 1
                }
            }
        });
    }

    pub(super) fn complete_path_suggestion(
        &mut self,
        direction: PathSuggestionDirection,
    ) -> Task<Message> {
        if self.path_suggestions.is_empty() {
            return Task::none();
        }

        if self.path_suggestion_selection.is_none() {
            self.path_suggestion_selection = Some(0);
        } else if direction == PathSuggestionDirection::Previous {
            self.move_path_suggestion_selection(direction);
        }

        let Some(path) = self
            .path_suggestion_selection
            .and_then(|index| self.path_suggestions.get(index))
            .cloned()
        else {
            return Task::none();
        };

        self.path_input = completed_path_text(&path);
        let request = self.next_path_suggestion_request();
        Task::batch([
            path_suggestions_command(self.active_pane_id(), request),
            iced::widget::operation::move_cursor_to_end(path_input_id(self.active_pane_id())),
        ])
    }

    pub(super) fn accept_expanded_directory(
        &mut self,
        pane_id: crate::model::BrowserPaneId,
        path: PathBuf,
        scan: Result<DirectoryScan, String>,
    ) -> Task<Message> {
        if pane_id != self.active_pane_id() {
            let pending_error = {
                let Some(pane) = self.pane_by_id_mut(pane_id) else {
                    return Task::none();
                };
                let expanded =
                    pane.expanded_directories
                        .entry(path)
                        .or_insert_with(|| ExpandedDirectory {
                            entries: Vec::new(),
                            status: ExpandedDirectoryStatus::Loading,
                            is_expanded: true,
                            animation_progress: 0.0,
                        });

                let mut pending_error = None;
                match scan {
                    Ok(scan) => {
                        expanded.entries = scan.entries;
                        expanded.status = ExpandedDirectoryStatus::Loaded;
                    }
                    Err(error) => {
                        expanded.entries.clear();
                        expanded.status = ExpandedDirectoryStatus::Error;
                        pending_error = Some(error);
                    }
                }
                pane.sync_active_tab_state();
                pending_error
            };
            if let Some(error) = pending_error {
                self.error = Some(error);
            }
            return Task::none();
        }

        let expanded = self
            .expanded_directories
            .entry(path)
            .or_insert_with(|| ExpandedDirectory {
                entries: Vec::new(),
                status: ExpandedDirectoryStatus::Loading,
                is_expanded: true,
                animation_progress: 0.0,
            });

        match scan {
            Ok(scan) => {
                expanded.entries = scan.entries;
                expanded.status = ExpandedDirectoryStatus::Loaded;
            }
            Err(error) => {
                expanded.entries.clear();
                expanded.status = ExpandedDirectoryStatus::Error;
                self.error = Some(error);
            }
        }

        let command = self.focus_created_entry_for_rename();
        self.sync_active_tab_state();
        Task::batch([command, self.schedule_thumbnail_refresh()])
    }

    pub(crate) fn entry_for_path(&self, path: &Path) -> Option<&DirectoryEntry> {
        self.entries
            .iter()
            .chain(
                self.expanded_directories
                    .values()
                    .flat_map(|expanded| expanded.entries.iter()),
            )
            .find(|entry| entry.path == path)
    }

    pub(super) fn entry_kind_recursive(&self, path: &Path) -> Option<FileKind> {
        self.entry_for_path(path).map(|entry| entry.kind)
    }

    fn path_from_input(&self) -> Option<PathBuf> {
        let trimmed = self.path_input.trim();
        if trimmed.is_empty() {
            return None;
        }

        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            Some(path)
        } else {
            Some(self.current_dir.join(path))
        }
    }

    fn next_path_suggestion_request(&mut self) -> PathSuggestionRequest {
        self.path_suggestion_generation = self.path_suggestion_generation.wrapping_add(1);
        PathSuggestionRequest {
            input: self.path_input.clone(),
            current_dir: self.current_dir.clone(),
            generation: self.path_suggestion_generation,
        }
    }

    fn active_path_suggestion_request_matches(&self, request: &PathSuggestionRequest) -> bool {
        path_suggestion_request_matches_state(
            request,
            &self.path_input,
            &self.current_dir,
            self.path_suggestion_generation,
        )
    }

    fn clear_selection_context(&mut self) {
        self.selected = None;
        self.selected_paths.clear();
        self.selection_anchor = None;
        self.drag_selection_anchor = None;
        self.column_resize_drag = None;
        self.file_drag = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.hovered_entry = None;
        self.hovered_sidebar = None;
        self.cursor_paste_directory = None;
        self.cursor_search_directory = None;
        self.last_click = None;
        self.clear_preview();
        self.context_menu = None;
        self.renaming = None;
        self.selection_marquee = None;
    }

    fn clear_transient_interaction_state(&mut self) {
        self.drag_selection_anchor = None;
        self.column_resize_drag = None;
        self.file_drag = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.hovered_entry = None;
        self.hovered_sidebar = None;
        self.cursor_paste_directory = None;
        self.cursor_search_directory = None;
        self.last_click = None;
        self.clear_preview();
        self.context_menu = None;
        self.renaming = None;
        self.selection_marquee = None;
    }

    fn refresh_expanded_directory_commands(&mut self) -> Vec<Task<Message>> {
        let paths = self
            .expanded_directories
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for path in &paths {
            if let Some(expanded) = self.expanded_directories.get_mut(path) {
                expanded.status = ExpandedDirectoryStatus::Loading;
                expanded.is_expanded = true;
                expanded.animation_progress = 1.0;
            }
        }

        paths
            .into_iter()
            .map(|path| {
                load_expanded_directory_command(self.active_pane_id(), path, self.options.clone())
            })
            .collect()
    }
}

fn path_suggestion_input_stabilization_command(
    pane_id: crate::model::BrowserPaneId,
    request: PathSuggestionRequest,
) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(PATH_SUGGESTION_INPUT_STABILIZATION_DELAY).await;
            request
        },
        move |request| Message::PathInputStabilized(pane_id, request),
    )
}

fn path_suggestion_request_matches_state(
    request: &PathSuggestionRequest,
    input: &str,
    current_dir: &Path,
    generation: u64,
) -> bool {
    request.input == input
        && request.current_dir.as_path() == current_dir
        && request.generation == generation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_suggestion_request_generation_rejects_repeated_stale_input() {
        let current_dir = PathBuf::from("/tmp");
        let request = PathSuggestionRequest {
            input: "docs".to_owned(),
            current_dir: current_dir.clone(),
            generation: 4,
        };

        assert!(!path_suggestion_request_matches_state(
            &request,
            "docs",
            &current_dir,
            5
        ));
        assert!(path_suggestion_request_matches_state(
            &request,
            "docs",
            &current_dir,
            4
        ));
    }

    #[test]
    fn stale_path_input_stabilization_keeps_current_suggestions() {
        let current_suggestion = PathBuf::from("/tmp/current");
        let mut browser = browser_with_path_suggestion_state(
            "docs",
            2,
            vec![current_suggestion.clone()],
            Some(0),
        );
        let stale_request = path_suggestion_request("docs", &browser.current_dir, 1);

        assert!(!browser.active_path_suggestion_request_matches(&stale_request));
        let _command = browser
            .load_stable_path_suggestions(crate::model::BrowserPaneId::PRIMARY, stale_request);

        assert_eq!(browser.path_suggestions, vec![current_suggestion]);
        assert_eq!(browser.path_suggestion_selection, Some(0));
        assert_eq!(browser.path_suggestion_generation, 2);
    }

    #[test]
    fn stale_path_suggestions_loaded_keeps_current_suggestions() {
        let current_suggestion = PathBuf::from("/tmp/current");
        let mut browser = browser_with_path_suggestion_state(
            "docs",
            2,
            vec![current_suggestion.clone()],
            Some(0),
        );
        let stale_request = path_suggestion_request("docs", &browser.current_dir, 1);

        let _command = browser.accept_path_suggestions(
            crate::model::BrowserPaneId::PRIMARY,
            stale_request,
            vec![PathBuf::from("/tmp/stale")],
        );

        assert_eq!(browser.path_suggestions, vec![current_suggestion]);
        assert_eq!(browser.path_suggestion_selection, Some(0));
    }

    #[test]
    fn empty_path_input_clears_suggestions_and_advances_generation() {
        let mut browser = browser_with_path_suggestion_state(
            "docs",
            9,
            vec![PathBuf::from("/tmp/current")],
            Some(0),
        );

        let _command = browser.update_path_input("  ".to_owned());

        assert_eq!(browser.path_input, "  ");
        assert!(browser.path_suggestions.is_empty());
        assert_eq!(browser.path_suggestion_selection, None);
        assert_eq!(browser.path_suggestion_generation, 10);
    }

    fn browser_with_path_suggestion_state(
        input: &str,
        generation: u64,
        suggestions: Vec<PathBuf>,
        selection: Option<usize>,
    ) -> FileBrowser {
        let (mut browser, _) = FileBrowser::new(crate::config::default_user_config());
        browser.current_dir = PathBuf::from("/tmp");
        browser.path_input = input.to_owned();
        browser.path_suggestion_generation = generation;
        browser.path_suggestions = suggestions;
        browser.path_suggestion_selection = selection;
        browser
    }

    fn path_suggestion_request(
        input: &str,
        current_dir: &Path,
        generation: u64,
    ) -> PathSuggestionRequest {
        PathSuggestionRequest {
            input: input.to_owned(),
            current_dir: current_dir.to_path_buf(),
            generation,
        }
    }
}
