use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use file_core::{is_supported_audio_path, is_supported_video_path, DirectoryEntry, FileKind};
use iced::Task;

use super::paths::{self, PasteTargetMode};
use super::{
    FileBrowser, PendingKeyboardColumnFocus, DOUBLE_CLICK_THRESHOLD,
    POINTER_DRAG_ACTIVATION_DISTANCE,
};
use crate::commands::{
    image_preview_dimensions_command, load_expanded_directory_command, open_file_command,
    open_terminal_command, preview_command, start_audio_preview_command,
};
use crate::model::{
    trash_location_path, AudioPreviewPlayback, BrowserPaneId, ColumnEntryBounds, ContextMenuState,
    ExpandedDirectory, ExpandedDirectoryStatus, FileContextMenuState, FileDragPhase, FileDragState,
    FileDragTarget, LastActivationClick, Message, NavigationMode, PreviewState,
    PreviewWindowProfile, SelectionMarquee, SelectionMarqueePhase, SelectionMarqueeSource,
    TransferConflictMode,
};
use crate::operation_queue::QueuedTransfer;
use crate::shortcuts::FileSelectionDirection;

#[cfg(test)]
mod activation_tests;
mod clipboard;
mod conflict;

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
            self.update_open_column_directory_for_entry(&path);
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
            self.open_column_for_directory(path)
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

    pub(super) fn handle_entry_right_clicked(&mut self, path: PathBuf) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        self.select_context_menu_target(path.clone());
        self.clear_preview();
        self.drag_selection_anchor = None;
        self.file_drag = None;
        self.context_menu = Some(ContextMenuState::FileArea(FileContextMenuState {
            target: Some(path.clone()),
            target_is_directory: self.entry_kind(&path) == Some(FileKind::Directory),
            paste_directory: self.entry_parent_directory(&path),
            position: self.cursor_position,
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
            position: self.cursor_position,
        }));
        rename_command
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

    pub(super) fn update_file_drag(&mut self, position: iced::Point) {
        let Some(file_drag) = &mut self.file_drag else {
            return;
        };
        let FileDragPhase::WaitingForMovement { origin } = file_drag.phase else {
            return;
        };

        let delta_x = position.x - origin.x;
        let delta_y = position.y - origin.y;
        if delta_x * delta_x + delta_y * delta_y
            >= POINTER_DRAG_ACTIVATION_DISTANCE * POINTER_DRAG_ACTIVATION_DISTANCE
        {
            file_drag.phase = FileDragPhase::Dragging;
        }
    }

    pub(super) fn finish_drag_selection(
        &mut self,
        release_directory: Option<PathBuf>,
    ) -> Task<Message> {
        let column_blank_click = self.selection_marquee.as_ref().and_then(|marquee| {
            if marquee.is_selecting() {
                return None;
            }
            match &marquee.source {
                SelectionMarqueeSource::PaneBlank => None,
                SelectionMarqueeSource::ColumnBlank { directory } => Some(directory.clone()),
            }
        });
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.sidebar_bookmark_drop_slot = None;
        if let Some(directory) = column_blank_click {
            return self.handle_column_blank_clicked(directory);
        }
        let Some(file_drag) = self.file_drag.take() else {
            return Task::none();
        };

        if !file_drag.is_dragging() {
            self.finish_stationary_file_drag(file_drag);
            return Task::none();
        }

        let cursor_fallback_directory = release_directory
            .clone()
            .or_else(|| self.file_drag_drop_directory_at_cursor());
        let Some(target) = resolve_file_drag_target(
            &file_drag.sources,
            release_directory,
            file_drag.target,
            cursor_fallback_directory,
        ) else {
            return Task::none();
        };

        match target {
            FileDragTarget::Directory(target_directory) => {
                self.move_dragged_files(file_drag.sources, target_directory)
            }
            FileDragTarget::SidebarBookmarkSlot(slot) => {
                self.add_dragged_sidebar_bookmark(slot, file_drag.sources)
            }
        }
    }

    fn start_file_drag(
        &mut self,
        pressed_path: PathBuf,
        column_directories_snapshot: Vec<PathBuf>,
    ) {
        self.sidebar_bookmark_drop_slot = None;
        if self.is_trash_view {
            self.file_drag = None;
            return;
        }

        let sources = self.selected_paths_for_operation();
        self.file_drag = (!sources.is_empty()).then_some(FileDragState {
            sources,
            pressed_path,
            target: None,
            phase: FileDragPhase::WaitingForMovement {
                origin: self.cursor_position,
            },
            column_directories_snapshot,
        });
    }

    fn finish_stationary_file_drag(&mut self, file_drag: FileDragState) {
        if file_drag.sources.len() > 1
            && file_drag
                .sources
                .iter()
                .any(|source| source == &file_drag.pressed_path)
        {
            self.select_path(file_drag.pressed_path);
        }
    }

    fn set_file_drag_target(&mut self, directory: PathBuf) {
        if let Some(file_drag) = &mut self.file_drag {
            file_drag.target = Some(FileDragTarget::Directory(directory));
        }
    }

    fn clear_file_drag_target(&mut self) {
        if let Some(file_drag) = &mut self.file_drag {
            file_drag.target = None;
        }
    }

    fn clear_file_drag_target_if_matching(&mut self, directory: &Path) {
        if let Some(file_drag) = &mut self.file_drag {
            if matches!(file_drag.target.as_ref(), Some(FileDragTarget::Directory(target)) if target == directory)
            {
                file_drag.target = None;
            }
        }
    }

    pub(super) fn file_drag_release_directory_for_entry(
        &self,
        pane_id: BrowserPaneId,
        path: &Path,
    ) -> Option<PathBuf> {
        self.cursor_paste_directory_for_entry_in_pane(pane_id, path)
    }

    pub(super) fn file_drag_release_directory_for_drop_target(
        &self,
        pane_id: BrowserPaneId,
        directory: PathBuf,
    ) -> Option<PathBuf> {
        self.pane_accepts_file_drag(pane_id).then_some(directory)
    }

    fn file_drag_drop_directory_at_cursor(&self) -> Option<PathBuf> {
        let pane_id = self.pane_id_at_position(self.cursor_position)?;
        if pane_id == self.active_pane_id() {
            return None;
        }

        let pane = self.pane_view(pane_id)?;
        (!pane.is_trash_view).then(|| pane.current_dir.clone())
    }

    fn move_dragged_files(
        &mut self,
        sources: Vec<PathBuf>,
        target_directory: PathBuf,
    ) -> Task<Message> {
        let transfers = paths::transfer_targets(&target_directory, &sources, PasteTargetMode::Move)
            .into_iter()
            .filter(|(source, target)| source != target && !target.starts_with(source))
            .map(|(source, target)| QueuedTransfer::new(source, target))
            .collect::<Vec<_>>();

        if transfers.is_empty() {
            return Task::none();
        }

        let open_drop_target = if sources
            .first()
            .and_then(|source| source.parent())
            .is_some_and(|source_parent| {
                target_directory != source_parent && target_directory.starts_with(source_parent)
            }) {
            self.select_path(target_directory.clone());
            self.open_column_for_directory(target_directory)
        } else {
            Task::none()
        };
        Task::batch([
            open_drop_target,
            self.enqueue_or_confirm_transfers(TransferConflictMode::Move, transfers),
        ])
    }

    pub(super) fn extend_drag_selection_to(&mut self, path: PathBuf) {
        let Some(anchor) = self.drag_selection_anchor.clone() else {
            if self.selection_marquee.is_some() {
                self.drag_selection_anchor = Some(path.clone());
                self.select_drag_range(path.clone(), path, self.keyboard_modifiers.control());
            }
            return;
        };
        self.select_drag_range(anchor, path, self.keyboard_modifiers.control());
    }

    pub(super) fn select_all_visible(&mut self) -> Task<Message> {
        let paths = self.visible_entry_paths();
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

    pub(super) fn activate_selected_path(&mut self) -> Task<Message> {
        if !self.file_browser_content_shortcuts_enabled() {
            return Task::none();
        }
        let Some(path) = self.selected.clone() else {
            return Task::none();
        };
        self.activate_path(path)
    }

    pub(super) fn move_file_selection(
        &mut self,
        direction: FileSelectionDirection,
    ) -> Task<Message> {
        if !self.file_browser_content_shortcuts_enabled() {
            return Task::none();
        }

        match direction {
            FileSelectionDirection::Up => {
                self.move_file_selection_vertically(SelectionStep::Previous)
            }
            FileSelectionDirection::Down => {
                self.move_file_selection_vertically(SelectionStep::Next)
            }
            FileSelectionDirection::Left => self.move_file_selection_to_parent_column(),
            FileSelectionDirection::Right => self.move_file_selection_to_child_column(),
        }
    }

    fn move_file_selection_vertically(&mut self, step: SelectionStep) -> Task<Message> {
        let directory = self
            .selected
            .as_ref()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| self.current_dir.clone());
        let paths = self.entry_paths_in_directory(&directory);
        let Some(target) = stepped_selection_target(&paths, self.selected.as_deref(), step) else {
            return Task::none();
        };

        self.select_path_from_keyboard(target.clone());
        self.open_column_for_keyboard_selection(target)
    }

    fn move_file_selection_to_parent_column(&mut self) -> Task<Message> {
        let Some(selected) = self.selected.clone() else {
            return Task::none();
        };
        let Some(parent) = selected.parent().map(Path::to_path_buf) else {
            return Task::none();
        };
        if parent == self.current_dir || self.entry_kind(&parent) != Some(FileKind::Directory) {
            return Task::none();
        }

        self.column_return_targets
            .insert(parent.clone(), selected.clone());
        self.select_path_from_keyboard(parent.clone());
        self.focus_column_containing_path(&parent)
    }

    fn move_file_selection_to_child_column(&mut self) -> Task<Message> {
        let Some(selected) = self.selected.clone() else {
            return self.move_file_selection_vertically(SelectionStep::Next);
        };
        if self.entry_kind(&selected) != Some(FileKind::Directory) {
            return Task::none();
        }

        let preferred_child = self.column_return_targets.get(&selected).cloned();
        let open_command = self.open_column_for_directory(selected.clone());
        if let Some(path) = self.keyboard_child_focus_target(&selected, preferred_child.as_deref())
        {
            self.pending_keyboard_column_focus = None;
            self.select_path_from_keyboard(path.clone());
            return Task::batch([open_command, self.open_column_for_keyboard_selection(path)]);
        }

        if self.directory_children_are_loading(&selected) {
            self.pending_keyboard_column_focus = Some(PendingKeyboardColumnFocus {
                pane_id: self.active_pane_id(),
                directory: selected,
                preferred_child,
            });
        } else {
            self.pending_keyboard_column_focus = None;
        }
        open_command
    }

    pub(super) fn complete_pending_keyboard_column_focus(
        &mut self,
        directory: &Path,
    ) -> Task<Message> {
        let Some(pending) = self.pending_keyboard_column_focus.clone() else {
            return Task::none();
        };
        if pending.pane_id != self.active_pane_id() || pending.directory != directory {
            return Task::none();
        }

        self.pending_keyboard_column_focus = None;
        if self.selected.as_deref() != Some(directory) {
            return Task::none();
        }

        let Some(path) =
            self.keyboard_child_focus_target(directory, pending.preferred_child.as_deref())
        else {
            return Task::none();
        };

        self.select_path_from_keyboard(path.clone());
        self.open_column_for_keyboard_selection(path)
    }

    fn keyboard_child_focus_target(
        &self,
        directory: &Path,
        preferred_child: Option<&Path>,
    ) -> Option<PathBuf> {
        let paths = self.entry_paths_in_directory(directory);
        if let Some(preferred_child) = preferred_child {
            if paths.iter().any(|path| path == preferred_child) {
                return Some(preferred_child.to_path_buf());
            }
        }
        paths.into_iter().next()
    }

    fn directory_children_are_loading(&self, directory: &Path) -> bool {
        self.expanded_directories
            .get(directory)
            .is_some_and(|expanded| matches!(expanded.status, ExpandedDirectoryStatus::Loading))
    }

    fn open_column_for_keyboard_selection(&mut self, path: PathBuf) -> Task<Message> {
        if self.entry_kind(&path) == Some(FileKind::Directory) {
            self.open_column_for_directory(path)
        } else {
            self.update_open_column_directory_for_entry(&path);
            Task::none()
        }
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

    fn select_path_from_keyboard(&mut self, path: PathBuf) {
        self.select_path(path.clone());
        self.selection_anchor = Some(path);
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.file_drag = None;
        self.pending_keyboard_column_focus = None;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
    }

    pub(super) fn request_preview(&mut self) -> Task<Message> {
        if self.preview.is_some() {
            self.context_menu = None;
            return self.close_preview_window();
        }

        self.open_preview()
    }

    fn activate_path(&mut self, path: PathBuf) -> Task<Message> {
        if self.is_trash_view {
            return Task::none();
        }

        match self.entry_kind(&path) {
            Some(FileKind::Directory) => self.navigate_to(path, NavigationMode::RecordHistory),
            Some(_) | None => open_file_command(path, self.terminal_emulator),
        }
    }

    pub(super) fn open_terminal_here(&mut self, directory: PathBuf) -> Task<Message> {
        self.context_menu = None;
        if self.is_trash_view {
            return Task::none();
        }
        open_terminal_command(directory, self.terminal_emulator)
    }

    fn open_column_for_directory(&mut self, path: PathBuf) -> Task<Message> {
        if self.is_trash_view {
            return Task::none();
        }

        if self.entry_kind(&path) != Some(FileKind::Directory) {
            return Task::none();
        }
        self.set_deepest_open_column_directory(Some(path.clone()));

        if let Some(expanded) = self.expanded_directories.get_mut(&path) {
            expanded.is_expanded = true;
            expanded.animation_progress = 1.0;
            return self.focus_latest_column();
        }

        self.expanded_directories.insert(
            path.clone(),
            ExpandedDirectory {
                entries: Vec::new(),
                status: ExpandedDirectoryStatus::Loading,
                is_expanded: true,
                animation_progress: 1.0,
            },
        );
        Task::batch([
            load_expanded_directory_command(self.active_pane_id(), path, self.options.clone()),
            self.focus_latest_column(),
        ])
    }

    fn open_preview(&mut self) -> Task<Message> {
        self.context_menu = None;

        let Some(path) = self.selected.clone() else {
            let window_command = self.ensure_preview_window(PreviewWindowProfile::Regular);
            self.clear_preview();
            self.preview = Some(PreviewState::Error("Select an item to preview".to_owned()));
            return window_command;
        };

        let kind = self.entry_kind(&path).unwrap_or(FileKind::Other);
        let is_audio_preview = kind == FileKind::File && is_supported_audio_path(&path);
        let is_video_preview = kind == FileKind::File && is_supported_video_path(&path);
        let is_image_preview = kind == FileKind::File
            && thumbnails::is_supported_thumbnail_path(&path)
            && !is_video_preview;
        if is_image_preview {
            let close_window_command = self.close_preview_window();
            self.preview = Some(PreviewState::Loading(path.clone()));
            self.error = None;
            return Task::batch([close_window_command, image_preview_dimensions_command(path)]);
        }
        if is_video_preview {
            let close_window_command = self.close_preview_window();
            self.preview = Some(PreviewState::Loading(path.clone()));
            self.error = None;
            return Task::batch([
                close_window_command,
                preview_command(path, kind, self.options.clone()),
            ]);
        }

        let window_profile = if is_audio_preview {
            PreviewWindowProfile::Audio
        } else {
            PreviewWindowProfile::Regular
        };
        let window_command = self.ensure_preview_window(window_profile);
        self.clear_preview();
        self.preview = Some(PreviewState::Loading(path.clone()));
        self.error = None;
        if is_audio_preview {
            self.audio_preview = Some(AudioPreviewPlayback::loading(path.clone()));
            return Task::batch([
                window_command,
                preview_command(path.clone(), kind, self.options.clone()),
                start_audio_preview_command(path),
            ]);
        }
        Task::batch([
            window_command,
            preview_command(path, kind, self.options.clone()),
        ])
    }

    pub(super) fn entry_kind(&self, path: &Path) -> Option<FileKind> {
        self.entry_kind_recursive(path)
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

    fn selected_paths_for_operation(&self) -> Vec<PathBuf> {
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
        let mut paths = Vec::new();
        for entry in &self.entries {
            self.push_visible_entry_paths(entry, &mut paths);
        }
        paths
    }

    fn push_visible_entry_paths(&self, entry: &DirectoryEntry, paths: &mut Vec<PathBuf>) {
        paths.push(entry.path.clone());
        let Some(expanded) = self
            .expanded_directories
            .get(&entry.path)
            .filter(|expanded| expanded.is_expanded || expanded.animation_progress > 0.0)
        else {
            return;
        };
        if matches!(expanded.status, ExpandedDirectoryStatus::Loaded) {
            for child in &expanded.entries {
                self.push_visible_entry_paths(child, paths);
            }
        }
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

#[derive(Debug, Clone, Copy)]
enum SelectionStep {
    Previous,
    Next,
}

fn stepped_selection_target(
    paths: &[PathBuf],
    selected: Option<&Path>,
    step: SelectionStep,
) -> Option<PathBuf> {
    if paths.is_empty() {
        return None;
    }

    let current = selected.and_then(|selected| paths.iter().position(|path| path == selected));
    let index = match (current, step) {
        (Some(index), SelectionStep::Previous) => index.saturating_sub(1),
        (Some(index), SelectionStep::Next) => (index + 1).min(paths.len() - 1),
        (None, SelectionStep::Previous) => paths.len() - 1,
        (None, SelectionStep::Next) => 0,
    };
    paths.get(index).cloned()
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

fn resolve_file_drag_target(
    sources: &[PathBuf],
    release_directory: Option<PathBuf>,
    target: Option<FileDragTarget>,
    fallback_directory: Option<PathBuf>,
) -> Option<FileDragTarget> {
    if let Some(release_directory) = release_directory {
        return Some(FileDragTarget::Directory(release_directory));
    }

    match target {
        Some(FileDragTarget::Directory(target_directory)) => {
            if file_drag_directory_target_needs_fallback(sources, &target_directory) {
                if let Some(fallback_directory) = fallback_directory {
                    Some(FileDragTarget::Directory(fallback_directory))
                } else {
                    Some(FileDragTarget::Directory(target_directory))
                }
            } else {
                Some(FileDragTarget::Directory(target_directory))
            }
        }
        Some(FileDragTarget::SidebarBookmarkSlot(slot)) => {
            Some(FileDragTarget::SidebarBookmarkSlot(slot))
        }
        None => fallback_directory.map(FileDragTarget::Directory),
    }
}

fn file_drag_directory_target_needs_fallback(sources: &[PathBuf], target: &Path) -> bool {
    sources.iter().any(|source| {
        source == target
            || target.starts_with(source)
            || source.parent().is_some_and(|parent| parent == target)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use file_core::{DirectoryEntry, EntryMetadata, FileKind};
    use iced::{Point, Rectangle};

    use crate::{
        app::FileBrowser,
        config,
        model::{
            BrowserPaneId, ColumnEntryBounds, ContextMenuState, FileDragTarget, SelectionMarquee,
            SelectionMarqueePhase, SelectionMarqueeSource,
        },
    };

    fn test_entry(path: PathBuf, kind: FileKind) -> DirectoryEntry {
        DirectoryEntry::new(
            path,
            kind,
            EntryMetadata {
                len: 0,
                modified: None,
                readonly: false,
            },
            false,
            false,
            false,
        )
    }

    fn active_selection_marquee(
        current: Point,
        base_selection: HashSet<PathBuf>,
        preserve_existing: bool,
    ) -> SelectionMarquee {
        SelectionMarquee {
            start: Point::new(0.0, 0.0),
            current,
            source: SelectionMarqueeSource::PaneBlank,
            phase: SelectionMarqueePhase::Selecting,
            base_selection,
            preserve_existing,
        }
    }

    fn entry_bounds(path: PathBuf, x: f32, y: f32, width: f32, height: f32) -> ColumnEntryBounds {
        ColumnEntryBounds {
            pane_id: BrowserPaneId::PRIMARY,
            path,
            bounds: Rectangle {
                x,
                y,
                width,
                height,
            },
        }
    }

    #[test]
    fn rectangles_intersect_only_when_areas_overlap() {
        let first = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 20.0,
        };

        assert!(super::rectangles_intersect(
            first,
            Rectangle {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
            }
        ));
        assert!(!super::rectangles_intersect(
            first,
            Rectangle {
                x: 20.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            }
        ));
    }

    #[test]
    fn drag_target_falls_back_from_source_parent_to_pane_directory() {
        let source = PathBuf::from("/right/file.txt");
        let source_parent = PathBuf::from("/right");
        let fallback = PathBuf::from("/left");

        let target = super::resolve_file_drag_target(
            &[source],
            None,
            Some(FileDragTarget::Directory(source_parent)),
            Some(fallback.clone()),
        );

        assert!(matches!(target, Some(FileDragTarget::Directory(path)) if path == fallback));
    }

    #[test]
    fn drag_target_keeps_hovered_directory_in_target_pane() {
        let source = PathBuf::from("/right/file.txt");
        let hovered = PathBuf::from("/left/folder");
        let fallback = PathBuf::from("/left");

        let target = super::resolve_file_drag_target(
            &[source],
            None,
            Some(FileDragTarget::Directory(hovered.clone())),
            Some(fallback),
        );

        assert!(matches!(target, Some(FileDragTarget::Directory(path)) if path == hovered));
    }

    #[test]
    fn drag_target_prefers_release_column_over_stale_hover_target() {
        let source = PathBuf::from("/right/file.txt");
        let stale_hover_target = PathBuf::from("/right");
        let release_column = PathBuf::from("/left/actual-column");
        let fallback = PathBuf::from("/left");

        let target = super::resolve_file_drag_target(
            &[source],
            Some(release_column.clone()),
            Some(FileDragTarget::Directory(stale_hover_target)),
            Some(fallback),
        );

        assert!(matches!(target, Some(FileDragTarget::Directory(path)) if path == release_column));
    }

    #[test]
    fn drag_target_uses_pane_directory_when_hover_target_missing() {
        let source = PathBuf::from("/right/file.txt");
        let fallback = PathBuf::from("/left");

        let target = super::resolve_file_drag_target(&[source], None, None, Some(fallback.clone()));

        assert!(matches!(target, Some(FileDragTarget::Directory(path)) if path == fallback));
    }

    #[test]
    fn marquee_selection_uses_intersecting_bounds() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let current_dir = PathBuf::from("/workspace");
        let inside = current_dir.join("inside.txt");
        let outside = current_dir.join("outside.txt");
        browser.current_dir = current_dir;
        browser.entries = vec![
            test_entry(inside.clone(), FileKind::File),
            test_entry(outside.clone(), FileKind::File),
        ];
        browser.selection_marquee = Some(active_selection_marquee(
            Point::new(50.0, 50.0),
            HashSet::new(),
            false,
        ));

        let command = browser.update_selection_from_column_entry_bounds(vec![
            entry_bounds(inside.clone(), 10.0, 10.0, 20.0, 20.0),
            entry_bounds(outside.clone(), 70.0, 70.0, 20.0, 20.0),
        ]);
        drop(command);

        assert!(browser.is_path_selected(&inside));
        assert!(!browser.is_path_selected(&outside));
        assert_eq!(browser.selected.as_ref(), Some(&inside));
    }

    #[test]
    fn marquee_selection_preserves_existing_selection_when_requested() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let current_dir = PathBuf::from("/workspace");
        let preserved = current_dir.join("preserved.txt");
        let added = current_dir.join("added.txt");
        browser.current_dir = current_dir;
        browser.entries = vec![
            test_entry(preserved.clone(), FileKind::File),
            test_entry(added.clone(), FileKind::File),
        ];
        browser.selection_marquee = Some(active_selection_marquee(
            Point::new(50.0, 50.0),
            HashSet::from([preserved.clone()]),
            true,
        ));

        let command = browser.update_selection_from_column_entry_bounds(vec![entry_bounds(
            added.clone(),
            10.0,
            10.0,
            20.0,
            20.0,
        )]);
        drop(command);

        assert!(browser.is_path_selected(&preserved));
        assert!(browser.is_path_selected(&added));
    }

    #[test]
    fn clicking_current_column_blank_clears_existing_selection() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let current_dir = PathBuf::from("/workspace");
        let first = current_dir.join("first.txt");
        let second = current_dir.join("second.txt");
        let child_directory = current_dir.join("project");
        browser.current_dir = current_dir.clone();
        browser.entries = vec![
            test_entry(first.clone(), FileKind::File),
            test_entry(second.clone(), FileKind::File),
            test_entry(child_directory.clone(), FileKind::Directory),
        ];
        browser.selected = Some(second.clone());
        browser.selected_paths = HashSet::from([first, second]);
        browser.selection_anchor = Some(child_directory.clone());

        let command = browser.handle_column_blank_clicked(current_dir.clone());
        drop(command);

        assert!(browser.selected.is_none());
        assert!(browser.selected_paths.is_empty());
        assert!(browser.selection_anchor.is_none());
        assert_eq!(
            browser.path_input,
            crate::app::paths::path_text(&current_dir)
        );
    }

    #[test]
    fn clicking_child_column_blank_preserves_open_column_context() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let current_dir = PathBuf::from("/workspace");
        let first = current_dir.join("first.txt");
        let second = current_dir.join("second.txt");
        let child_directory = current_dir.join("project");
        browser.current_dir = current_dir;
        browser.entries = vec![
            test_entry(first.clone(), FileKind::File),
            test_entry(second.clone(), FileKind::File),
            test_entry(child_directory.clone(), FileKind::Directory),
        ];
        browser.selected = Some(second);
        browser.selected_paths = HashSet::from([first]);

        let command = browser.handle_column_blank_clicked(child_directory.clone());
        drop(command);

        assert_eq!(
            browser.deepest_open_column_directory.as_ref(),
            Some(&child_directory)
        );
        assert_eq!(browser.selected.as_ref(), Some(&child_directory));
        assert_eq!(
            browser.selected_paths,
            HashSet::from([child_directory.clone()])
        );
        assert_eq!(browser.selection_anchor.as_ref(), Some(&child_directory));
        assert_eq!(
            browser.path_input,
            crate::app::paths::path_text(&child_directory)
        );
    }

    #[test]
    fn pressing_current_column_blank_clears_selection_before_release() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let current_dir = PathBuf::from("/workspace");
        let first = current_dir.join("first.txt");
        let second = current_dir.join("second.txt");
        let child_directory = current_dir.join("project");
        browser.current_dir = current_dir.clone();
        browser.entries = vec![
            test_entry(first.clone(), FileKind::File),
            test_entry(second.clone(), FileKind::File),
            test_entry(child_directory.clone(), FileKind::Directory),
        ];
        browser.selected = Some(second.clone());
        browser.selected_paths = HashSet::from([first, second]);
        browser.selection_anchor = Some(child_directory.clone());

        let command = browser.start_column_blank_selection_marquee(current_dir.clone());
        drop(command);

        assert!(browser.selected.is_none());
        assert!(browser.selected_paths.is_empty());
        assert!(browser.selection_anchor.is_none());
        assert!(browser.selection_marquee.is_some());
        assert_eq!(
            browser.path_input,
            crate::app::paths::path_text(&current_dir)
        );
    }

    #[test]
    fn pressing_child_column_blank_preserves_open_column_before_release() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let current_dir = PathBuf::from("/workspace");
        let first = current_dir.join("first.txt");
        let second = current_dir.join("second.txt");
        let child_directory = current_dir.join("project");
        browser.current_dir = current_dir;
        browser.entries = vec![
            test_entry(first.clone(), FileKind::File),
            test_entry(second.clone(), FileKind::File),
            test_entry(child_directory.clone(), FileKind::Directory),
        ];
        browser.selected = Some(second);
        browser.selected_paths = HashSet::from([first]);

        let command = browser.start_column_blank_selection_marquee(child_directory.clone());
        drop(command);

        assert_eq!(
            browser.deepest_open_column_directory.as_ref(),
            Some(&child_directory)
        );
        assert_eq!(browser.selected.as_ref(), Some(&child_directory));
        assert_eq!(
            browser.selected_paths,
            HashSet::from([child_directory.clone()])
        );
        assert_eq!(browser.selection_anchor.as_ref(), Some(&child_directory));
        assert!(browser.selection_marquee.is_some());
        assert_eq!(
            browser.path_input,
            crate::app::paths::path_text(&child_directory)
        );
    }

    #[test]
    fn dragging_child_column_blank_preserves_open_column_context() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let current_dir = PathBuf::from("/workspace");
        let first = current_dir.join("first.txt");
        let second = current_dir.join("second.txt");
        let child_directory = current_dir.join("project");
        browser.current_dir = current_dir;
        browser.entries = vec![
            test_entry(first.clone(), FileKind::File),
            test_entry(second.clone(), FileKind::File),
            test_entry(child_directory.clone(), FileKind::Directory),
        ];
        browser.selected = Some(second);
        browser.selected_paths = HashSet::from([first]);

        let press_command = browser.start_column_blank_selection_marquee(child_directory.clone());
        drop(press_command);
        browser
            .selection_marquee
            .as_mut()
            .expect("selection marquee starts")
            .phase = SelectionMarqueePhase::Selecting;

        let drag_command = browser.update_selection_from_column_entry_bounds(Vec::new());
        drop(drag_command);

        assert_eq!(
            browser.deepest_open_column_directory.as_ref(),
            Some(&child_directory)
        );
        assert_eq!(
            crate::three_column_view::column_directories(&browser),
            vec![browser.current_dir.clone(), child_directory.clone()]
        );
        assert!(browser.selected.is_none());
        assert!(browser.selected_paths.is_empty());
        assert_eq!(
            browser.path_input,
            crate::app::paths::path_text(&browser.current_dir)
        );
    }

    #[test]
    fn releasing_selected_item_without_drag_collapses_multi_selection() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let current_dir = PathBuf::from("/workspace");
        let first = current_dir.join("first.txt");
        let second = current_dir.join("second.txt");
        browser.current_dir = current_dir;
        browser.entries = vec![
            test_entry(first.clone(), FileKind::File),
            test_entry(second.clone(), FileKind::File),
        ];
        browser.selected = Some(first.clone());
        browser.selected_paths = HashSet::from([first.clone(), second.clone()]);

        let press_command = browser.handle_column_entry_clicked(second.clone());
        drop(press_command);
        assert_eq!(
            browser.selected_paths,
            HashSet::from([first, second.clone()])
        );

        let release_command = browser.finish_drag_selection(None);
        drop(release_command);

        assert_eq!(browser.selected.as_ref(), Some(&second));
        assert_eq!(browser.selected_paths, HashSet::from([second.clone()]));
        assert_eq!(browser.selection_anchor.as_ref(), Some(&second));
    }

    #[test]
    fn right_clicking_directory_selects_menu_target_without_focusing_it() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let current_dir = PathBuf::from("/workspace");
        let directory = current_dir.join("project");
        browser.current_dir = current_dir.clone();
        browser.entries = vec![test_entry(directory.clone(), FileKind::Directory)];
        browser.selected = None;
        browser.selected_paths.clear();
        browser.expanded_directories.clear();

        let command = browser.handle_entry_right_clicked(directory.clone());
        drop(command);

        assert_eq!(browser.current_dir, current_dir);
        assert!(browser.selected.is_none());
        assert!(browser.is_path_selected(&directory));
        assert!(!browser.expanded_directories.contains_key(&directory));

        let ContextMenuState::FileArea(context_menu) =
            browser.context_menu.as_ref().expect("context menu opens")
        else {
            panic!("file context menu opens");
        };
        assert_eq!(context_menu.target.as_ref(), Some(&directory));
        assert!(context_menu.target_is_directory);
    }
}
