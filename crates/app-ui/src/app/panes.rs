use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use file_core::DirectoryEntry;
use iced::Point;

use super::tabs::{TabAnimationState, TabBarReveal};
use super::FileBrowser;
use crate::model::{
    BrowserPane, BrowserPaneId, BrowserPaneLayout, BrowserTab, BrowserViewMode,
    DirectoryLoadingPlaceholderEntry, ExpandedDirectory, FileDragState, SplitRegion,
};
use crate::thumbnail_cache::ColumnViewport;

const SPLIT_TARGET_MIN_CONTENT_WIDTH: f32 = 1.0;

#[derive(Clone, Copy)]
pub(crate) struct BrowserPaneView<'a> {
    pub(crate) id: BrowserPaneId,
    pub(crate) current_dir: &'a PathBuf,
    pub(crate) is_trash_view: bool,
    pub(crate) entries: &'a [DirectoryEntry],
    pub(crate) directory_loading_placeholder_entries: &'a [DirectoryLoadingPlaceholderEntry],
    pub(crate) selected: Option<&'a PathBuf>,
    pub(crate) selected_paths: &'a HashSet<PathBuf>,
    pub(crate) deepest_open_column_directory: Option<&'a PathBuf>,
    pub(crate) hovered_entry: Option<&'a PathBuf>,
    pub(crate) expanded_directories: &'a HashMap<PathBuf, ExpandedDirectory>,
    pub(crate) column_viewports: &'a HashMap<PathBuf, ColumnViewport>,
    pub(crate) view_mode: BrowserViewMode,
    pub(crate) tabs: &'a [BrowserTab],
    pub(crate) active_tab_id: usize,
    pub(crate) tab_animations: Option<&'a HashMap<usize, TabAnimationState>>,
    pub(crate) path_input: &'a str,
    pub(crate) path_suggestions: &'a [PathBuf],
    pub(crate) path_suggestion_selection: Option<usize>,
    pub(crate) is_loading: bool,
    pub(crate) renaming: Option<&'a PathBuf>,
    pub(crate) rename_input: &'a str,
    pub(crate) file_drag: Option<&'a FileDragState>,
    pub(crate) tab_bar_reveal_fraction: f32,
}

impl BrowserPaneView<'_> {
    pub(crate) fn is_path_selected(&self, path: &std::path::Path) -> bool {
        self.selected_paths.contains(path)
    }

    pub(crate) fn tab_bar_should_occupy_layout(&self) -> bool {
        self.tabs.len() > 1 || self.tab_bar_reveal_fraction > f32::EPSILON
    }

    pub(crate) fn tab_width_fraction(&self, tab_id: usize) -> f32 {
        self.tab_animations
            .and_then(|animations| animations.get(&tab_id))
            .map(|animation| animation.width_fraction())
            .unwrap_or(1.0)
    }

    pub(crate) fn tab_shift_offset(&self, tab_id: usize) -> f32 {
        self.tab_animations
            .and_then(|animations| animations.get(&tab_id))
            .map(|animation| animation.shift_offset())
            .unwrap_or(0.0)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SplitOverlayBounds {
    pub(crate) top_left: Point,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl FileBrowser {
    pub(crate) fn active_pane_id(&self) -> BrowserPaneId {
        self.pane_layout.active()
    }

    pub(super) fn pane_id_at_position(&self, position: Point) -> Option<BrowserPaneId> {
        let sidebar_width = self.sidebar_width;
        let content_width = self.split_content_width();
        let content_height = self.main_window_height.max(1.0);
        if position.x < sidebar_width
            || position.x > sidebar_width + content_width
            || position.y < 0.0
            || position.y > content_height
        {
            return None;
        }

        match self.pane_layout {
            BrowserPaneLayout::Single { active } => Some(active),
            BrowserPaneLayout::Split {
                axis: crate::model::SplitAxis::Horizontal,
                first,
                second,
                ..
            } => {
                let local_x = position.x - sidebar_width;
                if local_x < content_width / 2.0 {
                    Some(first)
                } else {
                    Some(second)
                }
            }
            BrowserPaneLayout::Split {
                axis: crate::model::SplitAxis::Vertical,
                first,
                second,
                ..
            } => {
                if position.y < content_height / 2.0 {
                    Some(first)
                } else {
                    Some(second)
                }
            }
        }
    }

    pub(crate) fn pane_view(&self, pane_id: BrowserPaneId) -> Option<BrowserPaneView<'_>> {
        if pane_id == self.active_pane_id() {
            return Some(BrowserPaneView {
                id: pane_id,
                current_dir: &self.current_dir,
                is_trash_view: self.is_trash_view,
                entries: &self.entries,
                directory_loading_placeholder_entries: &self.directory_loading_placeholder_entries,
                selected: self.selected.as_ref(),
                selected_paths: &self.selected_paths,
                deepest_open_column_directory: self.deepest_open_column_directory.as_ref(),
                hovered_entry: self.hovered_entry.as_ref(),
                expanded_directories: &self.expanded_directories,
                column_viewports: &self.column_viewports,
                view_mode: self.view_mode,
                tabs: &self.tabs,
                active_tab_id: self.active_tab_id,
                tab_animations: Some(&self.tab_animations),
                path_input: &self.path_input,
                path_suggestions: &self.path_suggestions,
                path_suggestion_selection: self.path_suggestion_selection,
                is_loading: self.is_loading,
                renaming: self.renaming.as_ref(),
                rename_input: &self.rename_input,
                file_drag: self.file_drag.as_ref(),
                tab_bar_reveal_fraction: self.tab_bar_reveal_fraction(),
            });
        }

        let pane = self.pane_by_id(pane_id)?;
        Some(BrowserPaneView {
            id: pane.id,
            current_dir: &pane.current_dir,
            is_trash_view: pane.is_trash_view,
            entries: &pane.entries,
            directory_loading_placeholder_entries: &pane.directory_loading_placeholder_entries,
            selected: pane.selected.as_ref(),
            selected_paths: &pane.selected_paths,
            deepest_open_column_directory: pane.deepest_open_column_directory.as_ref(),
            hovered_entry: None,
            expanded_directories: &pane.expanded_directories,
            column_viewports: &pane.column_viewports,
            view_mode: pane.view_mode,
            tabs: &pane.tabs,
            active_tab_id: pane.active_tab_id,
            tab_animations: None,
            path_input: &pane.path_input,
            path_suggestions: &pane.path_suggestions,
            path_suggestion_selection: pane.path_suggestion_selection,
            is_loading: pane.is_loading,
            renaming: None,
            rename_input: "",
            file_drag: None,
            tab_bar_reveal_fraction: if pane.tabs.len() > 1 { 1.0 } else { 0.0 },
        })
    }

    pub(super) fn pane_by_id(&self, pane_id: BrowserPaneId) -> Option<&BrowserPane> {
        self.panes.iter().find(|pane| pane.id == pane_id)
    }

    pub(super) fn pane_by_id_mut(&mut self, pane_id: BrowserPaneId) -> Option<&mut BrowserPane> {
        self.panes.iter_mut().find(|pane| pane.id == pane_id)
    }

    pub(super) fn sync_active_pane_state(&mut self) {
        let snapshot = self.capture_active_pane_snapshot();
        if let Some(pane) = self.pane_by_id_mut(snapshot.id) {
            *pane = snapshot;
        } else {
            self.panes.push(snapshot);
        }
    }

    pub(super) fn activate_pane(&mut self, pane_id: BrowserPaneId) {
        if pane_id == self.active_pane_id() || self.pane_by_id(pane_id).is_none() {
            return;
        }

        self.sync_active_tab_state();
        let Some(next_pane) = self.pane_by_id(pane_id).cloned() else {
            return;
        };
        self.pane_layout = self.pane_layout.with_active(pane_id);
        self.restore_pane_snapshot(next_pane);
    }

    pub(super) fn new_pane_id(&mut self) -> BrowserPaneId {
        let pane_id = BrowserPaneId(self.next_pane_id);
        self.next_pane_id += 1;
        pane_id
    }

    pub(super) fn split_region_at(&self, position: Point) -> Option<SplitRegion> {
        let content_width = self.split_content_width();
        if content_width <= SPLIT_TARGET_MIN_CONTENT_WIDTH || self.main_window_height <= 1.0 {
            return None;
        }
        let sidebar_width = self.sidebar_width;
        if position.x < sidebar_width || position.y < 0.0 {
            return None;
        }
        if position.x > sidebar_width + content_width || position.y > self.main_window_height {
            return None;
        }

        let local_x = ((position.x - sidebar_width) / content_width).clamp(0.0, 1.0);
        let local_y = (position.y / self.main_window_height).clamp(0.0, 1.0);
        let horizontal_bias = (local_x - 0.5).abs();
        let vertical_bias = (local_y - 0.5).abs();

        if horizontal_bias >= vertical_bias {
            if local_x < 0.5 {
                Some(SplitRegion::Left)
            } else {
                Some(SplitRegion::Right)
            }
        } else if local_y < 0.5 {
            Some(SplitRegion::Top)
        } else {
            Some(SplitRegion::Bottom)
        }
    }

    pub(crate) fn tab_split_overlay_bounds(&self) -> Option<SplitOverlayBounds> {
        let drag = self.tab_drag.as_ref()?;
        if !drag.is_dragging() {
            return None;
        }
        let target = drag.split_target?;
        let content_width = self.split_content_width();
        let content_height = self.main_window_height.max(1.0);
        let sidebar_width = self.sidebar_width;
        let half_width = content_width / 2.0;
        let half_height = content_height / 2.0;

        let bounds = match target.region {
            SplitRegion::Left => SplitOverlayBounds {
                top_left: Point::new(sidebar_width, 0.0),
                width: half_width,
                height: content_height,
            },
            SplitRegion::Right => SplitOverlayBounds {
                top_left: Point::new(sidebar_width + half_width, 0.0),
                width: half_width,
                height: content_height,
            },
            SplitRegion::Top => SplitOverlayBounds {
                top_left: Point::new(sidebar_width, 0.0),
                width: content_width,
                height: half_height,
            },
            SplitRegion::Bottom => SplitOverlayBounds {
                top_left: Point::new(sidebar_width, half_height),
                width: content_width,
                height: half_height,
            },
        };
        Some(bounds)
    }

    pub(super) fn capture_active_pane_snapshot(&self) -> BrowserPane {
        BrowserPane {
            id: self.active_pane_id(),
            current_dir: self.current_dir.clone(),
            is_trash_view: self.is_trash_view,
            entries: self.entries.clone(),
            directory_loading_placeholder_entries: self
                .directory_loading_placeholder_entries
                .clone(),
            trash_entries: self.trash_entries.clone(),
            selected: self.selected.clone(),
            selected_paths: self.selected_paths.clone(),
            selection_anchor: self.selection_anchor.clone(),
            deepest_open_column_directory: self.deepest_open_column_directory.clone(),
            expanded_directories: self.expanded_directories.clone(),
            view_mode: self.view_mode,
            column_browser_viewport: self.column_browser_viewport,
            column_viewports: self.column_viewports.clone(),
            tabs: self.tabs.clone(),
            active_tab_id: self.active_tab_id,
            path_input: self.path_input.clone(),
            path_suggestions: self.path_suggestions.clone(),
            path_suggestion_selection: self.path_suggestion_selection,
            path_suggestion_generation: self.path_suggestion_generation,
            directory_load_generation: self.directory_load_generation,
            directory_load_cancel: self.directory_load_cancel.clone(),
            back_stack: self.back_stack.clone(),
            forward_stack: self.forward_stack.clone(),
            is_loading: self.is_loading,
        }
    }

    pub(super) fn restore_pane_snapshot(&mut self, pane: BrowserPane) {
        self.current_dir = pane.current_dir;
        self.is_trash_view = pane.is_trash_view;
        self.entries = pane.entries;
        self.directory_loading_placeholder_entries = pane.directory_loading_placeholder_entries;
        self.trash_entries = pane.trash_entries;
        self.selected = pane.selected;
        self.selected_paths = pane.selected_paths;
        self.selection_anchor = pane.selection_anchor;
        self.deepest_open_column_directory = pane.deepest_open_column_directory;
        self.expanded_directories = pane.expanded_directories;
        self.view_mode = pane.view_mode;
        self.column_browser_viewport = pane.column_browser_viewport;
        self.column_viewports = pane.column_viewports;
        self.tabs = pane.tabs;
        self.active_tab_id = pane.active_tab_id;
        self.tab_animations.clear();
        self.path_input = pane.path_input;
        self.path_suggestions = pane.path_suggestions;
        self.path_suggestion_selection = pane.path_suggestion_selection;
        self.path_suggestion_generation = pane.path_suggestion_generation;
        self.directory_load_generation = pane.directory_load_generation;
        self.directory_load_cancel = pane.directory_load_cancel;
        self.back_stack = pane.back_stack;
        self.forward_stack = pane.forward_stack;
        self.is_loading = pane.is_loading;
        self.tab_bar_reveal = if self.tabs.len() > 1 {
            TabBarReveal::Visible
        } else {
            TabBarReveal::Hidden
        };
        self.hovered_entry = None;
        self.cursor_paste_directory = None;
        self.cursor_search_directory = None;
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.column_resize_drag = None;
        self.pending_keyboard_column_focus = None;
        self.last_activation_click = None;
    }

    fn split_content_width(&self) -> f32 {
        (self.main_window_width - self.sidebar_width).max(1.0)
    }
}
