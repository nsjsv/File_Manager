use std::path::{Path, PathBuf};

use file_core::FileKind;
use iced::Task;

use super::FileBrowser;
use crate::commands::load_expanded_directory_command;
use crate::model::{
    BrowserPaneId, BrowserViewMode, ExpandedDirectory, ExpandedDirectoryStatus, Message,
};

const LIST_DIRECTORY_ANIMATION_STEP: f32 = 0.18;

impl FileBrowser {
    pub(super) fn select_browser_view_mode(
        &mut self,
        pane_id: BrowserPaneId,
        view_mode: BrowserViewMode,
    ) -> Task<Message> {
        self.activate_pane(pane_id);
        if self.view_mode == view_mode {
            return Task::none();
        }

        self.view_mode = view_mode;
        self.hovered_entry = None;
        self.cursor_paste_directory = None;
        self.cursor_search_directory = None;
        self.selection_marquee = None;
        self.file_drag = None;
        self.pending_keyboard_column_focus = None;
        self.column_resize_drag = None;
        self.user_config.browser_view_mode = view_mode;
        self.sync_active_tab_state();
        Task::batch([
            self.persist_user_config_command(),
            self.schedule_thumbnail_refresh(),
            self.request_browser_session_save(),
        ])
    }

    pub(super) fn list_directory_animation_is_active(&self) -> bool {
        self.expanded_directories
            .values()
            .any(expanded_directory_is_animating)
            || self.panes.iter().any(|pane| {
                pane.expanded_directories
                    .values()
                    .any(expanded_directory_is_animating)
            })
    }

    pub(super) fn advance_list_directory_animations(&mut self) -> Task<Message> {
        let active_changed = advance_expanded_directories(&mut self.expanded_directories);
        for pane in &mut self.panes {
            let pane_changed = advance_expanded_directories(&mut pane.expanded_directories);
            if pane_changed {
                pane.sync_active_tab_state();
            }
        }
        if active_changed {
            self.sync_active_tab_state();
        }
        Task::none()
    }

    pub(super) fn toggle_list_directory(
        &mut self,
        pane_id: BrowserPaneId,
        path: PathBuf,
    ) -> Task<Message> {
        self.activate_pane(pane_id);
        self.toggle_list_directory_for_path(path)
    }

    pub(super) fn expand_selected_list_directory(&mut self) -> Task<Message> {
        let Some(selected) = self.selected.clone() else {
            return Task::none();
        };
        if self.entry_kind(&selected) != Some(FileKind::Directory) {
            return Task::none();
        }

        if self
            .expanded_directories
            .get(&selected)
            .is_some_and(|expanded| expanded.is_expanded && !expanded.is_collapsing)
        {
            if let Some(child) = self.first_visible_child_path(&selected) {
                self.select_path_from_keyboard(child);
            }
            return Task::none();
        }

        self.open_list_directory(selected)
    }

    pub(super) fn collapse_selected_list_directory_or_select_parent(&mut self) -> Task<Message> {
        let Some(selected) = self.selected.clone() else {
            return Task::none();
        };

        if self
            .expanded_directories
            .get(&selected)
            .is_some_and(|expanded| expanded.is_expanded && !expanded.is_collapsing)
        {
            return self.collapse_list_directory(selected);
        }

        let Some(parent) = selected.parent().map(Path::to_path_buf) else {
            return Task::none();
        };
        if parent == self.current_dir {
            return Task::none();
        }
        if !crate::visible_entries::entry_is_visible(
            &parent,
            &self.entries,
            &self.expanded_directories,
        ) {
            return Task::none();
        }

        self.select_path_from_keyboard(parent);
        self.sync_active_tab_state();
        self.schedule_thumbnail_refresh()
    }

    fn toggle_list_directory_for_path(&mut self, path: PathBuf) -> Task<Message> {
        if self.is_trash_view || self.entry_kind(&path) != Some(FileKind::Directory) {
            return Task::none();
        }

        if self
            .expanded_directories
            .get(&path)
            .is_some_and(|expanded| expanded.is_expanded && !expanded.is_collapsing)
        {
            return self.collapse_list_directory(path);
        }

        self.open_list_directory(path)
    }

    fn collapse_list_directory(&mut self, path: PathBuf) -> Task<Message> {
        if let Some(expanded) = self.expanded_directories.get_mut(&path) {
            Self::cancel_expanded_directory_load(expanded);
            expanded.is_collapsing = true;
            expanded.animation_progress = expanded.animation_progress.clamp(0.0, 1.0);
        }
        self.sync_active_tab_state();
        Task::batch([
            self.schedule_thumbnail_refresh(),
            self.request_browser_session_save(),
        ])
    }

    fn open_list_directory(&mut self, path: PathBuf) -> Task<Message> {
        if let Some(expanded) = self.expanded_directories.get_mut(&path) {
            expanded.is_expanded = true;
            expanded.is_collapsing = false;
            expanded.animation_progress = expanded.animation_progress.clamp(0.0, 1.0);
            self.sync_active_tab_state();
            return Task::batch([
                self.schedule_thumbnail_refresh(),
                self.request_browser_session_save(),
            ]);
        }

        let mut expanded = ExpandedDirectory {
            entries: Vec::new(),
            status: ExpandedDirectoryStatus::Loading,
            is_expanded: true,
            is_collapsing: false,
            animation_progress: 0.0,
            load_generation: 0,
            load_cancel: None,
        };
        let (request, cancellation) = Self::next_expanded_directory_load_request(
            self.active_pane_id(),
            path.clone(),
            &mut expanded,
        );
        self.expanded_directories.insert(path, expanded);
        self.sync_active_tab_state();
        Task::batch([
            load_expanded_directory_command(request, self.options.clone(), cancellation),
            self.schedule_thumbnail_refresh(),
            self.request_browser_session_save(),
        ])
    }

    fn first_visible_child_path(&self, directory: &Path) -> Option<PathBuf> {
        crate::visible_entries::visible_child_paths(
            directory,
            &self.current_dir,
            &self.entries,
            &self.expanded_directories,
        )
        .into_iter()
        .next()
    }
}

fn expanded_directory_is_animating(expanded: &ExpandedDirectory) -> bool {
    (expanded.is_expanded && !expanded.is_collapsing && expanded.animation_progress < 1.0)
        || expanded.is_collapsing
}

fn advance_expanded_directories(
    expanded_directories: &mut std::collections::HashMap<PathBuf, ExpandedDirectory>,
) -> bool {
    let mut changed = false;
    for expanded in expanded_directories.values_mut() {
        if expanded.is_collapsing {
            let next_progress =
                (expanded.animation_progress - LIST_DIRECTORY_ANIMATION_STEP).max(0.0);
            if (next_progress - expanded.animation_progress).abs() > f32::EPSILON {
                expanded.animation_progress = next_progress;
                changed = true;
            }
            if expanded.animation_progress <= f32::EPSILON {
                expanded.is_expanded = false;
                expanded.is_collapsing = false;
                changed = true;
            }
        } else if expanded.is_expanded && expanded.animation_progress < 1.0 {
            let next_progress =
                (expanded.animation_progress + LIST_DIRECTORY_ANIMATION_STEP).min(1.0);
            if (next_progress - expanded.animation_progress).abs() > f32::EPSILON {
                expanded.animation_progress = next_progress;
                changed = true;
            }
        }
    }
    changed
}
