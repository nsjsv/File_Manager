use std::path::PathBuf;

use file_core::FileKind;
use iced::{Point, Task};

use super::{FileBrowser, POINTER_DRAG_ACTIVATION_DISTANCE};
use crate::commands::save_sidebar_bookmarks_command;
use crate::model::{
    ContextMenuState, FileDragPhase, FileDragTarget, Message, NavigationMode,
    SidebarBookmarkContextMenuState, SidebarBookmarkDragState, SidebarBookmarkDropSlot,
    SidebarLocation, SidebarLocationKind,
};

const SIDEBAR_HEADER_HEIGHT: f32 = 24.0;
const SIDEBAR_LOCATION_ROW_HEIGHT: f32 = 28.0;
const SIDEBAR_ROW_SPACING: f32 = 6.0;
const SIDEBAR_VERTICAL_PADDING: f32 = 24.0;

impl FileBrowser {
    pub(crate) fn can_drop_sidebar_bookmark(&self) -> bool {
        let Some(file_drag) = &self.file_drag else {
            return false;
        };
        file_drag.is_dragging()
            && file_drag.sources.len() == 1
            && self.entry_kind(&file_drag.sources[0]) == Some(FileKind::Directory)
    }

    pub(super) fn update_sidebar_bookmark_drop_slot(&mut self, position: Point) -> Task<Message> {
        if !self.can_drop_sidebar_bookmark() {
            return self.clear_sidebar_bookmark_drop_slot();
        }

        let slot = if position.y <= self.sidebar_bookmark_drop_slot_midpoint() {
            SidebarBookmarkDropSlot::Top
        } else {
            SidebarBookmarkDropSlot::Bottom
        };
        if self.sidebar_bookmark_drop_slot != Some(slot) {
            self.clear_sidebar_bookmark_drop_target();
        }
        self.sidebar_bookmark_drop_slot = Some(slot);
        Task::none()
    }

    pub(super) fn clear_sidebar_bookmark_drop_slot(&mut self) -> Task<Message> {
        self.sidebar_bookmark_drop_slot = None;
        self.clear_sidebar_bookmark_drop_target();
        Task::none()
    }

    pub(super) fn handle_sidebar_bookmark_drop_slot_hovered(
        &mut self,
        slot: SidebarBookmarkDropSlot,
    ) -> Task<Message> {
        self.sidebar_bookmark_drop_slot = Some(slot);
        if self.can_drop_sidebar_bookmark() {
            if let Some(file_drag) = &mut self.file_drag {
                file_drag.target = Some(FileDragTarget::SidebarBookmarkSlot(slot));
            }
        }
        Task::none()
    }

    pub(super) fn handle_sidebar_bookmark_drop_slot_cleared(
        &mut self,
        slot: SidebarBookmarkDropSlot,
    ) -> Task<Message> {
        if let Some(file_drag) = &mut self.file_drag {
            if file_drag.target == Some(FileDragTarget::SidebarBookmarkSlot(slot)) {
                file_drag.target = None;
            }
        }
        Task::none()
    }

    pub(super) fn add_dragged_sidebar_bookmark(
        &mut self,
        slot: SidebarBookmarkDropSlot,
        sources: Vec<PathBuf>,
    ) -> Task<Message> {
        self.sidebar_bookmark_drop_slot = None;
        let Some(source) = sources.first().filter(|_| sources.len() == 1).cloned() else {
            return Task::none();
        };
        if self.entry_kind(&source) != Some(FileKind::Directory) || self.bookmark_exists(&source) {
            return Task::none();
        }

        let mut bookmarks = self.sidebar_bookmark_locations();
        let location = SidebarLocation {
            label: sidebar_bookmark_label(&source),
            path: source,
            kind: SidebarLocationKind::Bookmark,
        };
        match slot {
            SidebarBookmarkDropSlot::Top => bookmarks.insert(0, location),
            SidebarBookmarkDropSlot::Bottom => bookmarks.push(location),
        }
        self.replace_sidebar_bookmarks(bookmarks);
        self.save_sidebar_bookmarks()
    }

    pub(super) fn start_sidebar_bookmark_drag(&mut self, path: PathBuf) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        if !self.bookmark_exists(&path) {
            return rename_command;
        }
        self.context_menu = None;
        self.is_column_view_settings_open = false;
        self.file_drag = None;
        self.selection_marquee = None;
        self.sidebar_bookmark_drop_slot = None;
        self.sidebar_bookmark_drag = Some(SidebarBookmarkDragState {
            path,
            phase: FileDragPhase::WaitingForMovement {
                origin: self.cursor_position,
            },
            order_changed: false,
        });
        rename_command
    }

    pub(super) fn handle_sidebar_bookmark_right_clicked(&mut self, path: PathBuf) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        if !self.bookmark_exists(&path) {
            return rename_command;
        }

        self.clear_preview();
        self.is_column_view_settings_open = false;
        self.operation_queue.close_panel();
        self.file_drag = None;
        self.selection_marquee = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        self.context_menu = Some(ContextMenuState::SidebarBookmark(
            SidebarBookmarkContextMenuState {
                path,
                position: self.cursor_position,
            },
        ));
        rename_command
    }

    pub(super) fn delete_sidebar_bookmark(&mut self, path: PathBuf) -> Task<Message> {
        self.context_menu = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        if self.hovered_sidebar.as_ref() == Some(&path) {
            self.hovered_sidebar = None;
        }
        if !self.bookmark_exists(&path) {
            return Task::none();
        }

        let bookmarks = self
            .sidebar_bookmark_locations()
            .into_iter()
            .filter(|location| location.path != path)
            .collect::<Vec<_>>();
        self.replace_sidebar_bookmarks(bookmarks);
        self.save_sidebar_bookmarks()
    }

    pub(super) fn update_sidebar_bookmark_drag(&mut self, position: Point) {
        let Some(drag) = &mut self.sidebar_bookmark_drag else {
            return;
        };
        let FileDragPhase::WaitingForMovement { origin } = drag.phase else {
            return;
        };
        let delta_x = position.x - origin.x;
        let delta_y = position.y - origin.y;
        if delta_x * delta_x + delta_y * delta_y
            >= POINTER_DRAG_ACTIVATION_DISTANCE * POINTER_DRAG_ACTIVATION_DISTANCE
        {
            drag.phase = FileDragPhase::Dragging;
        }
    }

    pub(super) fn handle_sidebar_bookmark_entered(&mut self, path: PathBuf) -> Task<Message> {
        if self
            .sidebar_bookmark_drag
            .as_ref()
            .is_some_and(SidebarBookmarkDragState::is_dragging)
        {
            self.hovered_sidebar = Some(path.clone());
            self.reorder_sidebar_bookmark(path);
            Task::none()
        } else {
            self.handle_sidebar_hovered(path)
        }
    }

    pub(super) fn finish_sidebar_bookmark_drag(&mut self) -> Task<Message> {
        let Some(drag) = self.sidebar_bookmark_drag.take() else {
            return Task::none();
        };
        self.sidebar_bookmark_drop_slot = None;
        if drag.is_dragging() {
            if drag.order_changed {
                return self.save_sidebar_bookmarks();
            }
            return Task::none();
        }
        Task::batch([
            self.commit_rename_if_active(),
            self.navigate_to(drag.path, NavigationMode::RecordHistory),
        ])
    }

    fn reorder_sidebar_bookmark(&mut self, entered_path: PathBuf) {
        let Some(dragged_path) = self
            .sidebar_bookmark_drag
            .as_ref()
            .map(|drag| drag.path.clone())
        else {
            return;
        };
        if dragged_path == entered_path {
            return;
        }
        let mut bookmarks = self.sidebar_bookmark_locations();
        let Some(dragged_index) = bookmarks
            .iter()
            .position(|location| location.path == dragged_path)
        else {
            return;
        };
        let Some(entered_index) = bookmarks
            .iter()
            .position(|location| location.path == entered_path)
        else {
            return;
        };
        let dragged = bookmarks.remove(dragged_index);
        bookmarks.insert(entered_index, dragged);
        self.replace_sidebar_bookmarks(bookmarks);
        if let Some(drag) = &mut self.sidebar_bookmark_drag {
            drag.order_changed = true;
        }
    }

    fn sidebar_bookmark_locations(&self) -> Vec<SidebarLocation> {
        self.sidebar_locations
            .iter()
            .filter(|location| location.kind == SidebarLocationKind::Bookmark)
            .cloned()
            .collect()
    }

    fn replace_sidebar_bookmarks(&mut self, bookmarks: Vec<SidebarLocation>) {
        let mut locations = self
            .sidebar_locations
            .iter()
            .filter(|location| location.kind != SidebarLocationKind::Bookmark)
            .cloned()
            .collect::<Vec<_>>();
        locations.extend(bookmarks);
        self.sidebar_locations = locations;
    }

    fn save_sidebar_bookmarks(&self) -> Task<Message> {
        save_sidebar_bookmarks_command(self.sidebar_bookmark_locations())
    }

    fn clear_sidebar_bookmark_drop_target(&mut self) {
        if let Some(file_drag) = &mut self.file_drag {
            if matches!(
                file_drag.target.as_ref(),
                Some(FileDragTarget::SidebarBookmarkSlot(_))
            ) {
                file_drag.target = None;
            }
        }
    }

    fn sidebar_bookmark_drop_slot_midpoint(&self) -> f32 {
        let row_count = self.sidebar_locations.len() as f32 + 1.0;
        let child_count = row_count + 1.0;
        let spacing_height = (child_count - 1.0).max(0.0) * SIDEBAR_ROW_SPACING;
        let content_height = SIDEBAR_HEADER_HEIGHT
            + row_count * SIDEBAR_LOCATION_ROW_HEIGHT
            + spacing_height
            + SIDEBAR_VERTICAL_PADDING;

        content_height.min(self.main_window_height) / 2.0
    }

    fn bookmark_exists(&self, path: &PathBuf) -> bool {
        self.sidebar_locations.iter().any(|location| {
            location.kind == SidebarLocationKind::Bookmark && location.path == *path
        })
    }
}

fn sidebar_bookmark_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}
