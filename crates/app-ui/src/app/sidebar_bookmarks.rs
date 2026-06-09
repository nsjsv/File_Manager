use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use file_core::FileKind;
use iced::{Point, Task};

use super::{FileBrowser, POINTER_DRAG_ACTIVATION_DISTANCE};
use crate::commands::save_sidebar_bookmarks_command;
use crate::model::{
    ContextMenuState, FileDragPhase, FileDragTarget, Message, NavigationMode,
    SidebarBookmarkContextMenuState, SidebarBookmarkDragState, SidebarBookmarkDropSlot,
    SidebarLocation, SidebarLocationKind,
};
use crate::sidebar::sidebar_favorite_configs;

const SIDEBAR_HEADER_HEIGHT: f32 = 24.0;
const SIDEBAR_LOCATION_ROW_HEIGHT: f32 = 32.8;
const SIDEBAR_ROW_SPACING: f32 = 6.0;
const SIDEBAR_VERTICAL_PADDING: f32 = 24.0;
const SIDEBAR_BOOKMARK_MOTION_DURATION: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy)]
pub(crate) struct SidebarBookmarkMotionState {
    started_at: Instant,
    initial_y_offset: f32,
}

impl SidebarBookmarkMotionState {
    pub(crate) fn offset_y(self) -> f32 {
        let progress = sidebar_bookmark_ease_out_cubic(sidebar_bookmark_animation_progress(
            self.started_at,
            SIDEBAR_BOOKMARK_MOTION_DURATION,
        ));
        self.initial_y_offset * (1.0 - progress)
    }

    fn is_animating(self) -> bool {
        sidebar_bookmark_animation_progress(self.started_at, SIDEBAR_BOOKMARK_MOTION_DURATION) < 1.0
    }
}

impl FileBrowser {
    pub(crate) fn sidebar_bookmark_motion_offset(&self, path: &Path) -> f32 {
        if let Some(offset_y) = self.dragged_sidebar_bookmark_offset(path) {
            return offset_y;
        }

        self.sidebar_bookmark_motion
            .get(path)
            .map(|motion| motion.offset_y())
            .unwrap_or(0.0)
    }

    pub(super) fn sidebar_bookmark_motion_is_active(&self) -> bool {
        !self.sidebar_bookmark_motion.is_empty()
    }

    pub(super) fn advance_sidebar_bookmark_motion(&mut self) -> Task<Message> {
        self.prune_sidebar_bookmark_motion();
        Task::none()
    }

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
        if self.entry_kind(&source) != Some(FileKind::Directory)
            || self.sidebar_favorite_exists(&source)
        {
            return Task::none();
        }

        let mut favorites = self.sidebar_favorite_locations();
        let location = SidebarLocation {
            label: sidebar_bookmark_label(&source),
            path: source,
            kind: SidebarLocationKind::Bookmark,
        };
        match slot {
            SidebarBookmarkDropSlot::Top => favorites.insert(0, location),
            SidebarBookmarkDropSlot::Bottom => favorites.push(location),
        }
        self.replace_sidebar_favorites(favorites);
        self.save_sidebar_favorites()
    }

    pub(super) fn start_sidebar_bookmark_drag(&mut self, path: PathBuf) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        if !self.sidebar_favorite_exists(&path) {
            return rename_command;
        }
        self.context_menu = None;
        self.is_column_view_settings_open = false;
        self.file_drag = None;
        self.selection_marquee = None;
        self.sidebar_bookmark_drop_slot = None;
        self.sidebar_bookmark_motion.clear();
        let favorites = self.sidebar_favorite_locations();
        let source_index = favorites
            .iter()
            .position(|location| location.path == path)
            .unwrap_or(0);
        self.sidebar_bookmark_drag = Some(SidebarBookmarkDragState {
            path,
            origin: self.cursor_position,
            source_index,
            phase: FileDragPhase::WaitingForMovement {
                origin: self.cursor_position,
            },
            order_changed: false,
        });
        rename_command
    }

    pub(super) fn handle_sidebar_bookmark_right_clicked(&mut self, path: PathBuf) -> Task<Message> {
        let rename_command = self.commit_rename_if_active();
        if !self.sidebar_favorite_exists(&path) {
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
        self.sidebar_bookmark_motion.remove(path.as_path());
        if self.hovered_sidebar.as_ref() == Some(&path) {
            self.hovered_sidebar = None;
        }
        if !self.sidebar_favorite_exists(&path) {
            return Task::none();
        }

        let favorites = self
            .sidebar_favorite_locations()
            .into_iter()
            .filter(|location| location.path != path)
            .collect::<Vec<_>>();
        self.replace_sidebar_favorites(favorites);
        self.save_sidebar_favorites()
    }

    pub(super) fn update_sidebar_bookmark_drag(&mut self, position: Point) {
        let Some(drag) = self.sidebar_bookmark_drag.as_ref() else {
            return;
        };
        let origin = match drag.phase {
            FileDragPhase::WaitingForMovement { origin } => origin,
            FileDragPhase::Dragging => drag.origin,
        };
        let delta_x = position.x - origin.x;
        let delta_y = position.y - origin.y;
        let favorites = self.sidebar_favorite_locations();
        let current_index = favorites
            .iter()
            .position(|location| location.path == drag.path)
            .unwrap_or(drag.source_index);
        let projected_index =
            projected_sidebar_bookmark_index(drag.source_index, delta_y, favorites.len());
        let cursor_inside_sidebar_x = (0.0..=self.sidebar_width).contains(&position.x);
        let should_activate = matches!(drag.phase, FileDragPhase::WaitingForMovement { .. })
            && delta_x * delta_x + delta_y * delta_y
                >= POINTER_DRAG_ACTIVATION_DISTANCE * POINTER_DRAG_ACTIVATION_DISTANCE;

        if should_activate {
            let Some(drag) = &mut self.sidebar_bookmark_drag else {
                return;
            };
            drag.phase = FileDragPhase::Dragging;
        }

        if !cursor_inside_sidebar_x
            && self
                .sidebar_bookmark_drag
                .as_ref()
                .is_some_and(SidebarBookmarkDragState::is_dragging)
            && projected_index != current_index
        {
            if let Some(target) = favorites.get(projected_index) {
                self.hovered_sidebar = Some(target.path.clone());
                self.reorder_sidebar_bookmark(target.path.clone());
            }
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
            let release_offset = self.dragged_sidebar_bookmark_offset_for(&drag);
            if release_offset.abs() > f32::EPSILON {
                self.start_sidebar_bookmark_motion(vec![drag.path.clone()], release_offset);
            }

            if drag.order_changed {
                return self.save_sidebar_favorites();
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
        let mut favorites = self.sidebar_favorite_locations();
        let Some(dragged_index) = favorites
            .iter()
            .position(|location| location.path == dragged_path)
        else {
            return;
        };
        let Some(entered_index) = favorites
            .iter()
            .position(|location| location.path == entered_path)
        else {
            return;
        };
        let shifted_paths =
            shifted_sidebar_bookmark_paths(&favorites, dragged_index, entered_index);
        let row_stride = sidebar_bookmark_motion_stride();
        let shifted_offset = if dragged_index < entered_index {
            row_stride
        } else {
            -row_stride
        };

        let dragged = favorites.remove(dragged_index);
        favorites.insert(entered_index, dragged);
        self.replace_sidebar_favorites(favorites);
        self.start_sidebar_bookmark_motion(shifted_paths, shifted_offset);
        if let Some(drag) = &mut self.sidebar_bookmark_drag {
            drag.order_changed = true;
        }
    }

    fn start_sidebar_bookmark_motion(&mut self, paths: Vec<PathBuf>, initial_y_offset: f32) {
        let started_at = Instant::now();
        for path in paths {
            let current_y_offset = self
                .sidebar_bookmark_motion
                .get(path.as_path())
                .map(|motion| motion.offset_y())
                .unwrap_or(0.0);
            self.sidebar_bookmark_motion.insert(
                path,
                SidebarBookmarkMotionState {
                    started_at,
                    initial_y_offset: current_y_offset + initial_y_offset,
                },
            );
        }
    }

    fn prune_sidebar_bookmark_motion(&mut self) {
        let favorite_paths = self
            .sidebar_favorite_locations()
            .into_iter()
            .map(|location| location.path)
            .collect::<Vec<_>>();
        self.sidebar_bookmark_motion.retain(|path, motion| {
            favorite_paths.iter().any(|favorite| favorite == path) && motion.is_animating()
        });
    }

    fn dragged_sidebar_bookmark_offset(&self, path: &Path) -> Option<f32> {
        let drag = self.sidebar_bookmark_drag.as_ref()?;
        if drag.path != path || !drag.is_dragging() {
            return None;
        }
        Some(self.dragged_sidebar_bookmark_offset_for(drag))
    }

    fn dragged_sidebar_bookmark_offset_for(&self, drag: &SidebarBookmarkDragState) -> f32 {
        let cursor_offset = self.cursor_position.y - drag.origin.y;
        let favorites = self.sidebar_favorite_locations();
        let current_index = favorites
            .iter()
            .position(|location| location.path == drag.path)
            .unwrap_or(drag.source_index);
        let layout_offset =
            (drag.source_index as f32 - current_index as f32) * sidebar_bookmark_motion_stride();
        let raw_visual_offset = cursor_offset + layout_offset;
        let (min_visual_offset, max_visual_offset) =
            sidebar_bookmark_drag_offset_bounds(current_index, favorites.len());
        let visual_offset = raw_visual_offset.clamp(min_visual_offset, max_visual_offset);
        visual_offset
    }

    fn sidebar_favorite_locations(&self) -> Vec<SidebarLocation> {
        self.sidebar_locations
            .iter()
            .filter(|location| location.kind.is_user_favorite())
            .cloned()
            .collect()
    }

    fn replace_sidebar_favorites(&mut self, favorites: Vec<SidebarLocation>) {
        let mut locations = self
            .sidebar_locations
            .iter()
            .filter(|location| !location.kind.is_user_favorite())
            .cloned()
            .collect::<Vec<_>>();
        locations.extend(favorites);
        self.sidebar_locations = locations;
    }

    fn save_sidebar_favorites(&mut self) -> Task<Message> {
        let favorites = self.sidebar_favorite_locations();
        self.user_config.sidebar_favorites = Some(sidebar_favorite_configs(&favorites));
        Task::batch([
            save_sidebar_bookmarks_command(favorites),
            self.persist_user_config_command(),
        ])
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

    fn sidebar_favorite_exists(&self, path: &PathBuf) -> bool {
        self.sidebar_locations
            .iter()
            .any(|location| location.kind.is_user_favorite() && location.path == *path)
    }
}

fn shifted_sidebar_bookmark_paths(
    favorites: &[SidebarLocation],
    dragged_index: usize,
    entered_index: usize,
) -> Vec<PathBuf> {
    if dragged_index < entered_index {
        favorites[dragged_index + 1..=entered_index]
            .iter()
            .map(|location| location.path.clone())
            .collect()
    } else {
        favorites[entered_index..dragged_index]
            .iter()
            .map(|location| location.path.clone())
            .collect()
    }
}

fn sidebar_bookmark_motion_stride() -> f32 {
    SIDEBAR_LOCATION_ROW_HEIGHT + SIDEBAR_ROW_SPACING
}

fn sidebar_bookmark_drag_offset_bounds(current_index: usize, favorite_count: usize) -> (f32, f32) {
    let row_stride = sidebar_bookmark_motion_stride();
    let last_index = favorite_count.saturating_sub(1);
    let current_index = current_index.min(last_index);
    let min_offset = -(current_index as f32) * row_stride;
    let max_offset = last_index.saturating_sub(current_index) as f32 * row_stride;
    (min_offset, max_offset)
}

fn projected_sidebar_bookmark_index(
    source_index: usize,
    cursor_offset_y: f32,
    favorite_count: usize,
) -> usize {
    let last_index = favorite_count.saturating_sub(1);
    let projected = source_index as f32 + cursor_offset_y / sidebar_bookmark_motion_stride();
    projected.round().clamp(0.0, last_index as f32) as usize
}

fn sidebar_bookmark_animation_progress(started_at: Instant, duration: Duration) -> f32 {
    (started_at.elapsed().as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
}

fn sidebar_bookmark_ease_out_cubic(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    1.0 - (1.0 - progress).powi(3)
}

fn sidebar_bookmark_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}
