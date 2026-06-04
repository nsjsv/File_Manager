use std::path::{Path, PathBuf};

use file_core::{DirectoryEntry, DirectoryScan, FileKind, TrashScan};
use iced::widget::text_input;
use iced::Command;

use super::paths::{completed_path_text, path_text};
use super::FileBrowser;
use crate::commands::{
    load_directory_command, load_expanded_directory_command, load_trash_command,
    path_suggestions_command,
};
use crate::model::{
    trash_location_path, ExpandedDirectory, ExpandedDirectoryStatus, Message, NavigationMode,
    PathSuggestionDirection, TRASH_LOCATION_LABEL,
};
use crate::startup_trace;
use crate::view::path_input_id;

impl FileBrowser {
    pub(super) fn accept_directory_scan(&mut self, scan: DirectoryScan) -> Command<Message> {
        if scan.path != self.current_dir {
            return Command::none();
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
        Command::batch([command, self.schedule_thumbnail_refresh()])
    }

    pub(super) fn accept_trash_scan(&mut self, scan: TrashScan) -> Command<Message> {
        if !self.is_trash_view {
            return Command::none();
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
        self.schedule_thumbnail_refresh()
    }

    pub(super) fn navigate_to(&mut self, path: PathBuf, mode: NavigationMode) -> Command<Message> {
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
        load_directory_command(path, self.options.clone())
    }

    pub(super) fn open_trash_view(&mut self, mode: NavigationMode) -> Command<Message> {
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
        load_trash_command(self.options.clone())
    }

    pub(super) fn reload_current(&mut self) -> Command<Message> {
        if self.is_trash_view {
            self.path_input = TRASH_LOCATION_LABEL.to_owned();
            self.path_suggestions.clear();
            self.path_suggestion_selection = None;
            self.clear_transient_interaction_state();
            self.is_loading = true;
            self.error = None;
            self.expanded_directories.clear();
            return load_trash_command(self.options.clone());
        }

        self.path_input = path_text(&self.current_dir);
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        self.clear_transient_interaction_state();
        self.is_loading = true;
        self.error = None;

        let mut commands = self.refresh_expanded_directory_commands();
        commands.push(load_directory_command(
            self.current_dir.clone(),
            self.options.clone(),
        ));
        Command::batch(commands)
    }

    pub(super) fn reload_observed_directory(&mut self, path: PathBuf) -> Command<Message> {
        if self.is_trash_view {
            return Command::none();
        }

        if path == self.current_dir {
            return self.reload_current();
        }

        let Some(expanded) = self.expanded_directories.get_mut(&path) else {
            return Command::none();
        };

        expanded.status = ExpandedDirectoryStatus::Loading;
        expanded.is_expanded = true;
        expanded.animation_progress = 1.0;
        load_expanded_directory_command(path, self.options.clone())
    }

    pub(super) fn navigate_up(&mut self) -> Command<Message> {
        if self.is_trash_view {
            return Command::none();
        }

        if let Some(parent) = self.current_dir.parent() {
            self.navigate_to(parent.to_path_buf(), NavigationMode::RecordHistory)
        } else {
            Command::none()
        }
    }

    pub(super) fn navigate_back(&mut self) -> Command<Message> {
        if self.is_trash_view {
            if let Some(path) = self.back_stack.pop() {
                return self.navigate_to(path, NavigationMode::KeepHistory);
            }
            return Command::none();
        }

        if let Some(path) = self.back_stack.pop() {
            self.forward_stack.push(self.current_dir.clone());
            self.navigate_to(path, NavigationMode::KeepHistory)
        } else {
            Command::none()
        }
    }

    pub(super) fn navigate_forward(&mut self) -> Command<Message> {
        if let Some(path) = self.forward_stack.pop() {
            self.back_stack.push(self.current_dir.clone());
            self.navigate_to(path, NavigationMode::KeepHistory)
        } else {
            Command::none()
        }
    }

    pub(super) fn update_path_input(&mut self, value: String) -> Command<Message> {
        self.path_input = value.clone();
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        path_suggestions_command(value, self.current_dir.clone())
    }

    pub(super) fn submit_path_input(&mut self) -> Command<Message> {
        let Some(typed_path) = self.path_from_input() else {
            self.path_input = path_text(&self.current_dir);
            self.path_suggestions.clear();
            self.path_suggestion_selection = None;
            return Command::none();
        };

        let selected_suggestion = self
            .path_suggestion_selection
            .and_then(|index| self.path_suggestions.get(index))
            .cloned();

        let path = if let Some(path) = selected_suggestion {
            path
        } else if typed_path.exists() {
            typed_path
        } else {
            self.path_suggestions.first().cloned().unwrap_or(typed_path)
        };

        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        Command::batch([
            self.navigate_to(path, NavigationMode::RecordHistory),
            text_input::move_cursor_to_end(path_input_id()),
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
    ) -> Command<Message> {
        if self.path_suggestions.is_empty() {
            return Command::none();
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
            return Command::none();
        };

        self.path_input = completed_path_text(&path);
        Command::batch([
            path_suggestions_command(self.path_input.clone(), self.current_dir.clone()),
            text_input::move_cursor_to_end(path_input_id()),
        ])
    }

    pub(super) fn accept_expanded_directory(
        &mut self,
        path: PathBuf,
        scan: Result<DirectoryScan, String>,
    ) -> Command<Message> {
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
        Command::batch([command, self.schedule_thumbnail_refresh()])
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

    fn clear_selection_context(&mut self) {
        self.selected = None;
        self.selected_paths.clear();
        self.selection_anchor = None;
        self.drag_selection_anchor = None;
        self.column_resize_drag = None;
        self.file_drag = None;
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

    fn refresh_expanded_directory_commands(&mut self) -> Vec<Command<Message>> {
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
            .map(|path| load_expanded_directory_command(path, self.options.clone()))
            .collect()
    }
}
