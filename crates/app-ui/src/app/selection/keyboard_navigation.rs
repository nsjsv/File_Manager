use std::path::{Path, PathBuf};

use file_core::{
    is_supported_archive_path, is_supported_audio_path, is_supported_video_path, FileKind,
};
use iced::Task;

use super::super::{FileBrowser, PendingKeyboardColumnFocus};
use crate::animated_image_preview::is_animated_image_preview_path;
use crate::commands::{
    animated_image_preview_command, image_preview_dimensions_command,
    load_expanded_directory_command, open_file_command, open_terminal_command, preview_command,
    start_audio_preview_command,
};
use crate::model::{
    AudioPreviewPlayback, BrowserViewMode, ExpandedDirectory, ExpandedDirectoryStatus, Message,
    NavigationMode, PreviewState, PreviewWindowProfile,
};
use crate::shortcuts::FileSelectionDirection;

#[derive(Debug, Clone, Copy)]
enum SelectionStep {
    Previous,
    Next,
}

impl FileBrowser {
    pub(crate) fn activate_selected_path(&mut self) -> Task<Message> {
        if !self.file_browser_content_shortcuts_enabled() {
            return Task::none();
        }
        let Some(path) = self.selected.clone() else {
            return Task::none();
        };
        self.activate_path(path)
    }

    pub(crate) fn move_file_selection(
        &mut self,
        direction: FileSelectionDirection,
    ) -> Task<Message> {
        if !self.file_browser_content_shortcuts_enabled() {
            return Task::none();
        }

        match direction {
            FileSelectionDirection::Up if self.view_mode == BrowserViewMode::List => {
                self.move_file_selection_in_visible_list(SelectionStep::Previous)
            }
            FileSelectionDirection::Down if self.view_mode == BrowserViewMode::List => {
                self.move_file_selection_in_visible_list(SelectionStep::Next)
            }
            FileSelectionDirection::Up => {
                self.move_file_selection_vertically(SelectionStep::Previous)
            }
            FileSelectionDirection::Down => {
                self.move_file_selection_vertically(SelectionStep::Next)
            }
            FileSelectionDirection::Left if self.view_mode == BrowserViewMode::List => {
                self.collapse_selected_list_directory_or_select_parent()
            }
            FileSelectionDirection::Right if self.view_mode == BrowserViewMode::List => {
                self.expand_selected_list_directory()
            }
            FileSelectionDirection::Left => self.move_file_selection_to_parent_column(),
            FileSelectionDirection::Right => self.move_file_selection_to_child_column(),
        }
    }

    fn move_file_selection_in_visible_list(&mut self, step: SelectionStep) -> Task<Message> {
        let paths = self.visible_entry_paths();
        let Some(target) = stepped_selection_target(&paths, self.selected.as_deref(), step) else {
            return Task::none();
        };

        self.select_path_from_keyboard(target);
        self.schedule_thumbnail_refresh()
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

    pub(crate) fn complete_pending_keyboard_column_focus(
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

    pub(crate) fn select_path_from_keyboard(&mut self, path: PathBuf) {
        self.select_path(path.clone());
        self.selection_anchor = Some(path);
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.file_drag = None;
        self.pending_keyboard_column_focus = None;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
    }

    pub(crate) fn request_preview(&mut self) -> Task<Message> {
        if self.preview.is_some() {
            self.context_menu = None;
            return self.close_preview_window();
        }

        self.open_preview()
    }

    pub(crate) fn open_path(&mut self, path: PathBuf) -> Task<Message> {
        self.context_menu = None;
        self.activate_path(path)
    }

    pub(crate) fn activate_path(&mut self, path: PathBuf) -> Task<Message> {
        if self.is_trash_view {
            return Task::none();
        }

        match self.entry_kind(&path) {
            Some(FileKind::Directory) => self.navigate_to(path, NavigationMode::RecordHistory),
            Some(_) | None if is_supported_archive_path(&path) => {
                self.request_archive_extraction(path)
            }
            Some(_) | None => open_file_command(path, self.terminal_emulator),
        }
    }

    pub(crate) fn open_terminal_here(&mut self, directory: PathBuf) -> Task<Message> {
        self.context_menu = None;
        if self.is_trash_view {
            return Task::none();
        }
        open_terminal_command(directory, self.terminal_emulator)
    }

    pub(crate) fn open_column_for_directory(&mut self, path: PathBuf) -> Task<Message> {
        if self.is_trash_view {
            return Task::none();
        }

        if self.entry_kind(&path) != Some(FileKind::Directory) {
            return Task::none();
        }
        self.set_deepest_open_column_directory(Some(path.clone()));

        if let Some(expanded) = self.expanded_directories.get_mut(&path) {
            expanded.is_expanded = true;
            expanded.is_collapsing = false;
            expanded.animation_progress = 1.0;
            return self.focus_latest_column();
        }

        let mut expanded = ExpandedDirectory {
            entries: Vec::new(),
            status: ExpandedDirectoryStatus::Loading,
            is_expanded: true,
            is_collapsing: false,
            animation_progress: 1.0,
            load_generation: 0,
            load_cancel: None,
        };
        let (request, cancellation) = Self::next_expanded_directory_load_request(
            self.active_pane_id(),
            path.clone(),
            &mut expanded,
        );
        self.expanded_directories.insert(path, expanded);
        Task::batch([
            load_expanded_directory_command(request, self.options.clone(), cancellation),
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
        let is_animated_image_preview =
            kind == FileKind::File && is_animated_image_preview_path(&path);
        let is_image_preview = kind == FileKind::File
            && thumbnails::is_supported_thumbnail_path(&path)
            && !is_video_preview
            && !is_animated_image_preview;
        if is_animated_image_preview {
            let close_window_command = self.close_preview_window();
            self.preview = Some(PreviewState::Loading(path.clone()));
            self.error = None;
            let generation = self.next_animated_image_preview_generation();
            return Task::batch([
                close_window_command,
                animated_image_preview_command(path, generation),
            ]);
        }
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
