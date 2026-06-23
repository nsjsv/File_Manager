use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use file_core::FileKind;
use iced::Task;

use super::paths;
use super::{FileBrowser, DOUBLE_CLICK_THRESHOLD, POINTER_DRAG_ACTIVATION_DISTANCE};

use crate::model::{
    trash_location_path, BrowserPaneId, BrowserViewMode, ColumnEntryBounds, ContextMenuState,
    FileContextMenuExpansion, FileContextMenuState, FileDeleteAction, LastActivationClick, Message,
    SelectionMarquee, SelectionMarqueePhase, SelectionMarqueeSource,
};

#[cfg(test)]
mod activation_tests;
mod clipboard;
mod conflict;
mod drag;
mod keyboard_navigation;
#[cfg(test)]
mod tests;

#[cfg(test)]
use drag::resolve_file_drag_target;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryClickActivation {
    OpenColumn,
    SelectOnly,
}

impl FileBrowser {
    pub(crate) fn is_path_selected(&self, path: &Path) -> bool {
        self.selected_paths.contains(path)
    }

    pub(super) fn select_path(&mut self, path: PathBuf) {
        self.selected_paths.clear();
        self.selected_paths.insert(path.clone());
        self.selection_anchor = Some(path.clone());
        self.pending_keyboard_column_focus = None;
        self.focus_path(path);
    }

    pub(super) fn handle_column_entry_clicked(&mut self, path: PathBuf) -> Task<Message> {
        let column_directories_snapshot = crate::three_column_view::column_directories(self);
        self.handle_file_entry_clicked(
            path,
            EntryClickActivation::OpenColumn,
            column_directories_snapshot,
        )
    }

    pub(super) fn handle_list_entry_clicked(&mut self, path: PathBuf) -> Task<Message> {
        let column_directories_snapshot = Vec::new();
        self.handle_file_entry_clicked(
            path,
            EntryClickActivation::SelectOnly,
            column_directories_snapshot,
        )
    }

    fn handle_file_entry_clicked(
        &mut self,
        path: PathBuf,
        activation: EntryClickActivation,
        column_directories_snapshot: Vec<PathBuf>,
    ) -> Task<Message> {
        let was_selected = self.is_path_selected(&path);
        let rename_command = self.commit_rename_if_active();

        let now = Instant::now();
        let has_selection_modifier =
            self.keyboard_modifiers.control() || self.keyboard_modifiers.shift();
        let is_double_click = !has_selection_modifier
            && self
                .last_activation_click
                .as_ref()
                .is_some_and(|last_activation_click| {
                    last_activation_click.path == path
                        && now.duration_since(last_activation_click.at) <= DOUBLE_CLICK_THRESHOLD
                });

        if self.keyboard_modifiers.shift() {
            let anchor = self
                .selection_anchor
                .clone()
                .or_else(|| self.selected.clone())
                .unwrap_or_else(|| path.clone());
            self.select_range(
                anchor.clone(),
                path.clone(),
                self.keyboard_modifiers.control(),
            );
            self.selection_anchor = Some(anchor.clone());
            self.drag_selection_anchor = Some(anchor);
        } else if self.keyboard_modifiers.control() {
            self.toggle_path_selection(path.clone());
            self.selection_anchor = Some(path.clone());
            self.drag_selection_anchor = Some(path.clone());
            self.file_drag = None;
        } else {
            if was_selected {
                self.focus_path(path.clone());
                self.selection_anchor = Some(path.clone());
            } else {
                self.select_path(path.clone());
            }
            if activation == EntryClickActivation::OpenColumn {
                self.update_open_column_directory_for_entry(&path);
            }
            self.drag_selection_anchor = None;
            self.selection_marquee = None;
            self.start_file_drag(path.clone(), column_directories_snapshot);
        }

        self.last_activation_click = if has_selection_modifier {
            None
        } else {
            Some(LastActivationClick {
                path: path.clone(),
                at: now,
            })
        };

        let action_command = if is_double_click {
            self.drag_selection_anchor = None;
            self.file_drag = None;
            self.activate_path(path)
        } else if has_selection_modifier {
            Task::none()
        } else {
            match activation {
                EntryClickActivation::OpenColumn => self.open_column_for_directory(path),
                EntryClickActivation::SelectOnly => Task::none(),
            }
        };
        Task::batch([
            rename_command,
            action_command,
            self.schedule_thumbnail_refresh(),
        ])
    }

    pub(super) fn handle_entry_hovered(&mut self, path: PathBuf) -> Task<Message> {
        self.hovered_entry = Some(path.clone());
        let target_directory = self.cursor_paste_directory_for_entry(&path);
        self.cursor_paste_directory = Some(target_directory.clone());
        if self.file_drag.is_some() {
            self.set_file_drag_target(target_directory);
        } else if self.selection_marquee.is_none() {
            self.extend_drag_selection_to(path);
        }
        self.schedule_thumbnail_refresh()
    }

    pub(super) fn handle_entry_hover_cleared(&mut self, path: PathBuf) -> Task<Message> {
        if self.hovered_entry.as_ref() == Some(&path) {
            self.hovered_entry = None;
            self.cursor_paste_directory = Some(self.entry_parent_directory(&path));
        }
        Task::none()
    }

    pub(super) fn handle_drop_target_hovered(&mut self, directory: PathBuf) -> Task<Message> {
        self.hovered_entry = None;
        self.cursor_paste_directory = Some(directory.clone());
        if self.file_drag.is_some() {
            self.set_file_drag_target(directory);
        }
        Task::none()
    }

    pub(super) fn handle_drop_target_hover_cleared(&mut self, directory: PathBuf) -> Task<Message> {
        if self.cursor_paste_directory.as_ref() == Some(&directory) {
            self.cursor_paste_directory = None;
        }
        Task::none()
    }

    pub(super) fn handle_file_drag_entry_hovered_in_pane(
        &mut self,
        pane_id: BrowserPaneId,
        path: PathBuf,
    ) -> Task<Message> {
        if self.file_drag.is_none() {
            return Task::none();
        }

        let Some(target_directory) = self.cursor_paste_directory_for_entry_in_pane(pane_id, &path)
        else {
            self.clear_file_drag_target();
            return Task::none();
        };

        self.set_file_drag_target(target_directory);
        Task::none()
    }

    pub(super) fn handle_file_drag_entry_hover_cleared_in_pane(
        &mut self,
        pane_id: BrowserPaneId,
        path: PathBuf,
    ) -> Task<Message> {
        if self.file_drag.is_none() {
            return Task::none();
        }

        if let Some(target_directory) =
            self.cursor_paste_directory_for_entry_in_pane(pane_id, &path)
        {
            self.clear_file_drag_target_if_matching(&target_directory);
        }
        Task::none()
    }

    pub(super) fn handle_file_drag_drop_target_hovered_in_pane(
        &mut self,
        pane_id: BrowserPaneId,
        directory: PathBuf,
    ) -> Task<Message> {
        if self.file_drag.is_none() {
            return Task::none();
        }
        if !self.pane_accepts_file_drag(pane_id) {
            self.clear_file_drag_target();
            return Task::none();
        }

        self.set_file_drag_target(directory);
        Task::none()
    }

    pub(super) fn handle_file_drag_drop_target_hover_cleared_in_pane(
        &mut self,
        pane_id: BrowserPaneId,
        directory: PathBuf,
    ) -> Task<Message> {
        if self.file_drag.is_some() && self.pane_accepts_file_drag(pane_id) {
            self.clear_file_drag_target_if_matching(&directory);
        }
        Task::none()
    }

    pub(super) fn handle_sidebar_hovered(&mut self, path: PathBuf) -> Task<Message> {
        self.hovered_sidebar = Some(path.clone());
        self.cursor_paste_directory = None;
        if self.file_drag.is_some() {
            if path == trash_location_path() {
                self.clear_file_drag_target();
            } else {
                self.set_file_drag_target(path);
            }
        }
        Task::none()
    }

    pub(super) fn handle_sidebar_hover_cleared(&mut self, path: PathBuf) -> Task<Message> {
        if self.hovered_sidebar.as_ref() == Some(&path) {
            self.hovered_sidebar = None;
        }
        self.clear_file_drag_target_if_matching(&path);
        Task::none()
    }

    pub(super) fn clear_cursor_paste_target(&mut self) -> Task<Message> {
        self.cursor_paste_directory = None;
        Task::none()
    }

    pub(super) fn handle_column_blank_clicked(&mut self, directory: PathBuf) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        self.clear_preview();
        self.context_menu = None;
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.file_drag = None;
        self.focus_column_blank_context_or_clear_selection(directory);

        rename_command
    }

    pub(super) fn handle_column_placeholder_pressed(&mut self) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        self.clear_preview();
        self.context_menu = None;
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.file_drag = None;

        rename_command
    }

    pub(super) fn handle_entry_right_clicked(&mut self, path: PathBuf) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        self.select_context_menu_target(path.clone());
        let delete_action = self.file_delete_action_for_selection();
        self.clear_preview();
        self.drag_selection_anchor = None;
        self.file_drag = None;
        self.context_menu = Some(ContextMenuState::FileArea(FileContextMenuState {
            target: Some(path.clone()),
            target_is_directory: self.entry_kind(&path) == Some(FileKind::Directory),
            paste_directory: self.entry_parent_directory(&path),
            can_batch_rename: self.batch_rename_available_for_selection(),
            delete_action,
            position: self.cursor_position,
            expansion: FileContextMenuExpansion::None,
        }));
        rename_command
    }

    pub(super) fn handle_blank_area_right_clicked(&mut self, directory: PathBuf) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        self.clear_preview();
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.context_menu = Some(ContextMenuState::FileArea(FileContextMenuState {
            target: None,
            target_is_directory: false,
            paste_directory: directory,
            can_batch_rename: false,
            delete_action: FileDeleteAction::MoveToTrash,
            position: self.cursor_position,
            expansion: FileContextMenuExpansion::None,
        }));
        rename_command
    }

    pub(super) fn update_file_context_menu_expansion(
        &mut self,
        expansion: FileContextMenuExpansion,
    ) -> Task<Message> {
        if let Some(ContextMenuState::FileArea(menu)) = &mut self.context_menu {
            menu.expansion = expansion;
        }
        Task::none()
    }

    pub(super) fn start_selection_marquee(&mut self) -> Task<Message> {
        self.start_selection_marquee_from(SelectionMarqueeSource::PaneBlank)
    }

    pub(super) fn start_column_blank_selection_marquee(
        &mut self,
        directory: PathBuf,
    ) -> Task<Message> {
        if self.renaming.is_some() {
            return self.handle_column_blank_clicked(directory);
        }

        self.start_selection_marquee_from(SelectionMarqueeSource::ColumnBlank { directory })
    }

    fn start_selection_marquee_from(&mut self, source: SelectionMarqueeSource) -> Task<Message> {
        if self.renaming.is_some() {
            return self.commit_rename_if_active();
        }

        let column_context_directory = match &source {
            SelectionMarqueeSource::ColumnBlank { directory } => {
                self.column_blank_context_directory(directory)
            }
            SelectionMarqueeSource::PaneBlank => None,
        };
        self.clear_preview();
        self.context_menu = None;
        self.drag_selection_anchor = None;
        self.file_drag = None;
        let preserve_existing = self.keyboard_modifiers.control();
        let base_selection = self.selected_paths.clone();
        if column_context_directory.is_some() || !preserve_existing {
            self.set_deepest_open_column_directory(column_context_directory.clone());
        }
        if !preserve_existing {
            if let Some(directory) = column_context_directory {
                self.select_path(directory);
            } else {
                self.clear_column_blank_selection_context();
            }
        }
        self.selection_marquee = Some(SelectionMarquee {
            start: self.cursor_position,
            current: self.cursor_position,
            source,
            phase: SelectionMarqueePhase::WaitingForMovement,
            base_selection,
            preserve_existing,
        });
        Task::none()
    }

    fn focus_column_blank_context_or_clear_selection(&mut self, directory: PathBuf) {
        if let Some(directory) = self.column_blank_context_directory(&directory) {
            self.set_deepest_open_column_directory(Some(directory.clone()));
            self.select_path(directory);
        } else {
            self.clear_column_blank_selection_context();
        }
    }

    fn column_blank_context_directory(&self, directory: &Path) -> Option<PathBuf> {
        if directory == self.current_dir || self.entry_kind(directory) != Some(FileKind::Directory)
        {
            None
        } else {
            Some(directory.to_path_buf())
        }
    }

    fn update_open_column_directory_for_entry(&mut self, path: &Path) {
        let next_directory = match self.entry_kind(path) {
            Some(FileKind::Directory) => Some(path.to_path_buf()),
            _ => path.parent().map(Path::to_path_buf),
        };
        self.set_deepest_open_column_directory(next_directory);
    }

    fn set_deepest_open_column_directory(&mut self, directory: Option<PathBuf>) {
        self.deepest_open_column_directory = directory.filter(|directory| {
            directory != &self.current_dir && directory.starts_with(self.current_dir.as_path())
        });
    }

    fn clear_column_blank_selection_context(&mut self) {
        self.deepest_open_column_directory = None;
        self.selected_paths.clear();
        self.selected = None;
        self.selection_anchor = None;
        self.rename_input.clear();
        self.sync_path_input_to_current_directory();
    }

    pub(super) fn update_selection_marquee(&mut self, position: iced::Point) -> bool {
        if let Some(marquee) = &mut self.selection_marquee {
            marquee.current = position;
            if marquee.phase == SelectionMarqueePhase::WaitingForMovement
                && selection_marquee_distance_exceeded(marquee)
            {
                marquee.phase = SelectionMarqueePhase::Selecting;
            }
            return marquee.is_selecting();
        }
        false
    }

    pub(super) fn update_selection_from_column_entry_bounds(
        &mut self,
        bounds: Vec<ColumnEntryBounds>,
    ) -> Task<Message> {
        let Some(marquee) = self
            .selection_marquee
            .as_ref()
            .filter(|marquee| marquee.is_selecting())
        else {
            return Task::none();
        };
        let marquee_rectangle = marquee.rectangle();
        let active_pane_id = self.active_pane_id();
        let visible_paths = self
            .visible_entry_paths()
            .into_iter()
            .collect::<HashSet<_>>();
        let mut next_selection = if marquee.preserve_existing {
            marquee.base_selection.clone()
        } else {
            HashSet::new()
        };

        for entry_bounds in bounds {
            if entry_bounds.pane_id == active_pane_id
                && visible_paths.contains(&entry_bounds.path)
                && rectangles_intersect(marquee_rectangle, entry_bounds.bounds)
            {
                next_selection.insert(entry_bounds.path);
            }
        }

        self.selected_paths = next_selection;
        self.selected = self.last_visible_selected_path();
        if let Some(selected) = self.selected.clone() {
            self.sync_path_input_to_selected_directory(&selected);
            self.update_rename_input(&selected);
        } else {
            self.rename_input.clear();
            self.sync_path_input_to_current_directory();
        }
        Task::none()
    }

    pub(super) fn select_all_in_file_selection_scope(&mut self) -> Task<Message> {
        let paths = self.select_all_selection_scope_paths();
        self.selected_paths = paths.iter().cloned().collect::<HashSet<_>>();
        self.selection_anchor = paths.first().cloned();
        if let Some(path) = paths.first().cloned() {
            self.focus_path(path);
        } else {
            self.selected = None;
            self.rename_input.clear();
        }
        self.clear_preview();
        self.context_menu = None;
        Task::none()
    }

    fn select_all_selection_scope_paths(&self) -> Vec<PathBuf> {
        if self.view_mode == BrowserViewMode::Columns {
            return self.entry_paths_in_directory(&self.column_select_all_directory());
        }

        self.visible_entry_paths()
    }

    fn column_select_all_directory(&self) -> PathBuf {
        if let Some(path) = &self.hovered_entry {
            return self.entry_parent_directory(path);
        }

        if let Some(directory) = self.hovered_column_directory() {
            return directory;
        }

        self.selected
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.current_dir.clone())
    }

    fn hovered_column_directory(&self) -> Option<PathBuf> {
        let directory = self.cursor_paste_directory.as_ref()?;
        crate::three_column_view::column_directories(self)
            .into_iter()
            .find(|column_directory| column_directory == directory)
    }

    pub(super) fn entry_kind(&self, path: &Path) -> Option<FileKind> {
        self.entry_kind_recursive(path)
    }

    fn entry_paths_in_directory(&self, directory: &Path) -> Vec<PathBuf> {
        if directory == self.current_dir.as_path() {
            return self
                .entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect();
        }

        self.expanded_directories
            .get(directory)
            .map(|expanded| {
                expanded
                    .entries
                    .iter()
                    .map(|entry| entry.path.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn cursor_paste_directory_for_entry(&self, path: &Path) -> PathBuf {
        if self.entry_kind(path) == Some(FileKind::Directory) {
            path.to_path_buf()
        } else {
            self.entry_parent_directory(path)
        }
    }

    fn cursor_paste_directory_for_entry_in_pane(
        &self,
        pane_id: BrowserPaneId,
        path: &Path,
    ) -> Option<PathBuf> {
        let pane = self.pane_view(pane_id)?;
        if pane.is_trash_view {
            return None;
        }

        let entry_kind = pane
            .entries
            .iter()
            .chain(
                pane.expanded_directories
                    .values()
                    .flat_map(|expanded| expanded.entries.iter()),
            )
            .find(|entry| entry.path == path)
            .map(|entry| entry.kind);

        if entry_kind == Some(FileKind::Directory) {
            Some(path.to_path_buf())
        } else {
            Some(
                path.parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| pane.current_dir.clone()),
            )
        }
    }

    fn pane_accepts_file_drag(&self, pane_id: BrowserPaneId) -> bool {
        self.pane_view(pane_id)
            .is_some_and(|pane| !pane.is_trash_view)
    }

    fn entry_parent_directory(&self, path: &Path) -> PathBuf {
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.current_dir.clone())
    }

    fn select_context_menu_target(&mut self, path: PathBuf) {
        if self.is_path_selected(&path) {
            return;
        }

        self.selected_paths.clear();
        self.selected_paths.insert(path.clone());
        self.selection_anchor = Some(path);
    }

    fn toggle_path_selection(&mut self, path: PathBuf) {
        if self.selected_paths.remove(&path) {
            self.selected = self.last_visible_selected_path();
            if let Some(selected) = self.selected.clone() {
                self.focus_path(selected);
            } else {
                self.rename_input.clear();
                self.sync_path_input_to_current_directory();
            }
        } else {
            self.selected_paths.insert(path.clone());
            self.focus_path(path);
        }
        self.clear_preview();
        self.context_menu = None;
    }

    fn select_range(&mut self, anchor: PathBuf, target: PathBuf, preserve_existing: bool) {
        if !preserve_existing {
            self.selected_paths.clear();
        }

        for path in self.visible_range_paths(&anchor, &target) {
            self.selected_paths.insert(path);
        }

        self.focus_path(target);
    }

    fn select_drag_range(&mut self, anchor: PathBuf, target: PathBuf, preserve_existing: bool) {
        if !preserve_existing {
            self.selected_paths.clear();
        }

        for path in self.visible_range_paths(&anchor, &target) {
            self.selected_paths.insert(path);
        }
    }

    fn visible_range_paths(&self, anchor: &Path, target: &Path) -> Vec<PathBuf> {
        let paths = self.visible_entry_paths();
        let Some(anchor_index) = paths.iter().position(|path| path == anchor) else {
            return vec![target.to_path_buf()];
        };
        let Some(target_index) = paths.iter().position(|path| path == target) else {
            return vec![target.to_path_buf()];
        };

        let (start, end) = if anchor_index <= target_index {
            (anchor_index, target_index)
        } else {
            (target_index, anchor_index)
        };
        paths[start..=end].to_vec()
    }

    pub(super) fn selected_paths_for_operation(&self) -> Vec<PathBuf> {
        let mut paths = self
            .visible_entry_paths()
            .into_iter()
            .filter(|path| self.selected_paths.contains(path))
            .collect::<Vec<_>>();
        if paths.is_empty() {
            if let Some(path) = self.selected.clone() {
                paths.push(path);
            }
        }
        paths
    }

    fn file_delete_action_for_selection(&self) -> FileDeleteAction {
        let paths = self.selected_paths_for_operation();
        let has_network_path = paths.iter().any(|path| self.path_is_mounted_network(path));
        let has_local_path = paths.iter().any(|path| !self.path_is_mounted_network(path));
        match (has_network_path, has_local_path) {
            (true, false) => FileDeleteAction::DeletePermanently,
            (true, true) => FileDeleteAction::MixedSelection,
            _ => FileDeleteAction::MoveToTrash,
        }
    }

    fn selected_trash_entries_for_operation(&self) -> Vec<file_core::TrashRestoreEntry> {
        let paths = self.selected_paths_for_operation();
        self.trash_entries
            .iter()
            .filter(|trash_entry| paths.iter().any(|path| path == &trash_entry.trash_path))
            .map(|trash_entry| trash_entry.restore_entry())
            .collect()
    }

    fn last_visible_selected_path(&self) -> Option<PathBuf> {
        self.visible_entry_paths()
            .into_iter()
            .rev()
            .find(|path| self.selected_paths.contains(path))
    }

    fn visible_entry_paths(&self) -> Vec<PathBuf> {
        crate::visible_entries::visible_entry_paths(&self.entries, &self.expanded_directories)
    }

    fn focus_path(&mut self, path: PathBuf) {
        self.pending_keyboard_column_focus = None;
        self.sync_path_input_to_selected_directory(&path);
        self.update_rename_input(&path);
        self.selected = Some(path);
        self.clear_preview();
        self.context_menu = None;
    }

    fn sync_path_input_to_selected_directory(&mut self, path: &Path) {
        let directory = if self.entry_kind(path) == Some(FileKind::Directory) {
            path.to_path_buf()
        } else {
            self.entry_parent_directory(path)
        };
        self.path_input = paths::path_text(&directory);
    }

    fn sync_path_input_to_current_directory(&mut self) {
        self.path_input = paths::path_text(&self.current_dir);
    }

    fn update_rename_input(&mut self, path: &Path) {
        self.rename_input = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
}

fn selection_marquee_distance_exceeded(marquee: &SelectionMarquee) -> bool {
    let delta_x = marquee.current.x - marquee.start.x;
    let delta_y = marquee.current.y - marquee.start.y;
    delta_x * delta_x + delta_y * delta_y
        >= POINTER_DRAG_ACTIVATION_DISTANCE * POINTER_DRAG_ACTIVATION_DISTANCE
}

fn rectangles_intersect(first: iced::Rectangle, second: iced::Rectangle) -> bool {
    first.x < second.x + second.width
        && first.x + first.width > second.x
        && first.y < second.y + second.height
        && first.y + first.height > second.y
}
