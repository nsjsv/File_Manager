use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use file_core::{is_supported_audio_path, is_supported_video_path, DirectoryEntry, FileKind};
use iced::Command;

use super::paths::{self, PasteTargetMode};
use super::{FileBrowser, DOUBLE_CLICK_THRESHOLD};
use crate::commands::{
    image_preview_dimensions_command, load_expanded_directory_command, open_file_command,
    open_terminal_command, preview_command, start_audio_preview_command,
};
use crate::model::{
    trash_location_path, AudioPreviewPlayback, ContextMenuState, ExpandedDirectory,
    ExpandedDirectoryStatus, FileDragPhase, FileDragState, LastClick, Message, NavigationMode,
    PreviewState, PreviewWindowProfile, SelectionMarquee, TransferConflictMode,
};
use crate::operation_queue::QueuedTransfer;

mod clipboard;
mod conflict;

const FILE_DRAG_ACTIVATION_DISTANCE: f32 = 3.0;

impl FileBrowser {
    pub(crate) fn is_path_selected(&self, path: &Path) -> bool {
        self.selected_paths.contains(path)
    }

    pub(super) fn select_path(&mut self, path: PathBuf) {
        self.selected_paths.clear();
        self.selected_paths.insert(path.clone());
        self.selection_anchor = Some(path.clone());
        self.focus_path(path);
    }

    pub(super) fn handle_column_entry_clicked(&mut self, path: PathBuf) -> Command<Message> {
        self.is_column_view_settings_open = false;
        let column_directories_snapshot = crate::three_column_view::column_directories(self);
        let was_selected = self.is_path_selected(&path);
        let rename_command = self.commit_rename_if_active();

        let now = Instant::now();
        let has_selection_modifier =
            self.keyboard_modifiers.control() || self.keyboard_modifiers.shift();
        let is_double_click = !has_selection_modifier
            && self.last_click.as_ref().is_some_and(|last_click| {
                last_click.path == path
                    && now.duration_since(last_click.at) <= DOUBLE_CLICK_THRESHOLD
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
            self.drag_selection_anchor = None;
            self.selection_marquee = None;
            self.start_file_drag(column_directories_snapshot);
        }

        self.last_click = Some(LastClick {
            path: path.clone(),
            at: now,
        });

        let action_command = if is_double_click {
            self.drag_selection_anchor = None;
            self.file_drag = None;
            self.activate_path(path)
        } else {
            self.open_column_for_directory(path)
        };
        Command::batch([
            rename_command,
            action_command,
            self.schedule_thumbnail_refresh(),
        ])
    }

    pub(super) fn handle_entry_hovered(&mut self, path: PathBuf) -> Command<Message> {
        self.hovered_entry = Some(path.clone());
        let target_directory = self.cursor_paste_directory_for_entry(&path);
        self.cursor_paste_directory = Some(target_directory.clone());
        if self.file_drag.is_some() {
            self.set_file_drag_target(target_directory);
        } else {
            self.extend_drag_selection_to(path);
        }
        self.schedule_thumbnail_refresh()
    }

    pub(super) fn handle_entry_hover_cleared(&mut self, path: PathBuf) -> Command<Message> {
        if self.hovered_entry.as_ref() == Some(&path) {
            self.hovered_entry = None;
            self.cursor_paste_directory = Some(self.entry_parent_directory(&path));
        }
        Command::none()
    }

    pub(super) fn handle_drop_target_hovered(&mut self, directory: PathBuf) -> Command<Message> {
        self.hovered_entry = None;
        self.cursor_paste_directory = Some(directory.clone());
        if self.file_drag.is_some() {
            self.set_file_drag_target(directory);
        }
        Command::none()
    }

    pub(super) fn handle_drop_target_hover_cleared(
        &mut self,
        directory: PathBuf,
    ) -> Command<Message> {
        if self.cursor_paste_directory.as_ref() == Some(&directory) {
            self.cursor_paste_directory = None;
        }
        Command::none()
    }

    pub(super) fn handle_sidebar_hovered(&mut self, path: PathBuf) -> Command<Message> {
        self.hovered_sidebar = Some(path.clone());
        self.cursor_paste_directory = None;
        if self.file_drag.is_some() {
            if path == trash_location_path() {
                self.clear_file_drag_target();
            } else {
                self.set_file_drag_target(path);
            }
        }
        Command::none()
    }

    pub(super) fn handle_sidebar_hover_cleared(&mut self, path: PathBuf) -> Command<Message> {
        if self.hovered_sidebar.as_ref() == Some(&path) {
            self.hovered_sidebar = None;
        }
        self.clear_file_drag_target_if_matching(&path);
        Command::none()
    }

    pub(super) fn clear_cursor_paste_target(&mut self) -> Command<Message> {
        self.cursor_paste_directory = None;
        Command::none()
    }

    pub(super) fn handle_column_blank_clicked(&mut self, directory: PathBuf) -> Command<Message> {
        let rename_command = self.commit_rename_if_active();
        self.clear_preview();
        self.context_menu = None;
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.is_column_view_settings_open = false;
        self.file_drag = None;

        if directory == self.current_dir {
            self.selected_paths.clear();
            self.selected = None;
            self.selection_anchor = None;
            self.rename_input.clear();
            self.sync_path_input_to_current_directory();
        } else {
            self.select_path(directory);
        }

        rename_command
    }

    pub(super) fn handle_entry_right_clicked(&mut self, path: PathBuf) -> Command<Message> {
        let rename_command = self.commit_rename_if_active();
        self.select_context_menu_target(path.clone());
        self.clear_preview();
        self.drag_selection_anchor = None;
        self.file_drag = None;
        self.context_menu = Some(ContextMenuState {
            target: Some(path.clone()),
            target_is_directory: self.entry_kind(&path) == Some(FileKind::Directory),
            paste_directory: self.entry_parent_directory(&path),
            position: self.cursor_position,
        });
        rename_command
    }

    pub(super) fn handle_blank_area_right_clicked(
        &mut self,
        directory: PathBuf,
    ) -> Command<Message> {
        let rename_command = self.commit_rename_if_active();
        self.clear_preview();
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.context_menu = Some(ContextMenuState {
            target: None,
            target_is_directory: false,
            paste_directory: directory,
            position: self.cursor_position,
        });
        rename_command
    }

    pub(super) fn start_selection_marquee(&mut self) -> Command<Message> {
        if self.renaming.is_some() {
            return self.commit_rename_if_active();
        }

        self.clear_preview();
        self.context_menu = None;
        self.drag_selection_anchor = None;
        self.is_column_view_settings_open = false;
        self.file_drag = None;
        self.selection_marquee = Some(SelectionMarquee {
            start: self.cursor_position,
            current: self.cursor_position,
        });
        Command::none()
    }

    pub(super) fn update_selection_marquee(&mut self, position: iced::Point) {
        if let Some(marquee) = &mut self.selection_marquee {
            marquee.current = position;
        }
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
            >= FILE_DRAG_ACTIVATION_DISTANCE * FILE_DRAG_ACTIVATION_DISTANCE
        {
            file_drag.phase = FileDragPhase::Dragging;
        }
    }

    pub(super) fn finish_drag_selection(&mut self) -> Command<Message> {
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        let Some(file_drag) = self.file_drag.take() else {
            return Command::none();
        };

        if !file_drag.is_dragging() {
            return Command::none();
        }

        let Some(target_directory) = file_drag.target_directory else {
            return Command::none();
        };

        self.move_dragged_files(file_drag.sources, target_directory)
    }

    fn start_file_drag(&mut self, column_directories_snapshot: Vec<PathBuf>) {
        if self.is_trash_view {
            self.file_drag = None;
            return;
        }

        let sources = self.selected_paths_for_operation();
        self.file_drag = (!sources.is_empty()).then_some(FileDragState {
            sources,
            target_directory: None,
            phase: FileDragPhase::WaitingForMovement {
                origin: self.cursor_position,
            },
            column_directories_snapshot,
        });
    }

    fn set_file_drag_target(&mut self, directory: PathBuf) {
        if let Some(file_drag) = &mut self.file_drag {
            file_drag.target_directory = Some(directory);
        }
    }

    fn clear_file_drag_target(&mut self) {
        if let Some(file_drag) = &mut self.file_drag {
            file_drag.target_directory = None;
        }
    }

    fn clear_file_drag_target_if_matching(&mut self, directory: &Path) {
        if let Some(file_drag) = &mut self.file_drag {
            if file_drag.target_directory.as_deref() == Some(directory) {
                file_drag.target_directory = None;
            }
        }
    }

    fn move_dragged_files(
        &mut self,
        sources: Vec<PathBuf>,
        target_directory: PathBuf,
    ) -> Command<Message> {
        let transfers = paths::transfer_targets(&target_directory, &sources, PasteTargetMode::Move)
            .into_iter()
            .filter(|(source, target)| source != target && !target.starts_with(source))
            .map(|(source, target)| QueuedTransfer::new(source, target))
            .collect::<Vec<_>>();

        if transfers.is_empty() {
            return Command::none();
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
            Command::none()
        };
        Command::batch([
            open_drop_target,
            self.enqueue_or_confirm_transfers(TransferConflictMode::Move, transfers),
        ])
    }

    pub(super) fn extend_drag_selection_to(&mut self, path: PathBuf) {
        let Some(anchor) = self.drag_selection_anchor.clone() else {
            if self.selection_marquee.is_some() {
                self.drag_selection_anchor = Some(path.clone());
                self.select_range(path.clone(), path, self.keyboard_modifiers.control());
            }
            return;
        };
        self.select_range(anchor, path, self.keyboard_modifiers.control());
    }

    pub(super) fn select_all_visible(&mut self) -> Command<Message> {
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
        Command::none()
    }

    pub(super) fn request_preview(&mut self) -> Command<Message> {
        if self.preview.is_some() {
            self.context_menu = None;
            return self.close_preview_window();
        }

        self.open_preview()
    }

    fn activate_path(&mut self, path: PathBuf) -> Command<Message> {
        if self.is_trash_view {
            return Command::none();
        }

        match self.entry_kind(&path) {
            Some(FileKind::Directory) => self.navigate_to(path, NavigationMode::RecordHistory),
            Some(_) | None => open_file_command(path, self.terminal_emulator),
        }
    }

    pub(super) fn open_terminal_here(&mut self, directory: PathBuf) -> Command<Message> {
        self.context_menu = None;
        if self.is_trash_view {
            return Command::none();
        }
        open_terminal_command(directory, self.terminal_emulator)
    }

    fn open_column_for_directory(&mut self, path: PathBuf) -> Command<Message> {
        if self.is_trash_view {
            return Command::none();
        }

        if self.entry_kind(&path) != Some(FileKind::Directory) {
            return Command::none();
        }

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
        Command::batch([
            load_expanded_directory_command(path, self.options.clone()),
            self.focus_latest_column(),
        ])
    }

    fn open_preview(&mut self) -> Command<Message> {
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
            return Command::batch([close_window_command, image_preview_dimensions_command(path)]);
        }
        if is_video_preview {
            let close_window_command = self.close_preview_window();
            self.preview = Some(PreviewState::Loading(path.clone()));
            self.error = None;
            return Command::batch([
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
            return Command::batch([
                window_command,
                preview_command(path.clone(), kind, self.options.clone()),
                start_audio_preview_command(path),
            ]);
        }
        Command::batch([
            window_command,
            preview_command(path, kind, self.options.clone()),
        ])
    }

    fn entry_kind(&self, path: &Path) -> Option<FileKind> {
        self.entry_kind_recursive(path)
    }

    fn cursor_paste_directory_for_entry(&self, path: &Path) -> PathBuf {
        if self.entry_kind(path) == Some(FileKind::Directory) {
            path.to_path_buf()
        } else {
            self.entry_parent_directory(path)
        }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use file_core::{DirectoryEntry, EntryMetadata, FileKind};
    use iced::multi_window::Application;

    use crate::app::FileBrowser;

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

    #[test]
    fn right_clicking_directory_selects_menu_target_without_focusing_it() {
        let (mut browser, _) = <FileBrowser as Application>::new(());
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

        let context_menu = browser.context_menu.as_ref().expect("context menu opens");
        assert_eq!(context_menu.target.as_ref(), Some(&directory));
        assert!(context_menu.target_is_directory);
    }
}
