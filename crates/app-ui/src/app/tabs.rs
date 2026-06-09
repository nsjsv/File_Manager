use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iced::Task;

use super::paths::path_text;
use super::FileBrowser;
use crate::model::{
    BrowserPane, BrowserPaneId, BrowserPaneLayout, BrowserTab, FileDragPhase, Message,
    NavigationMode, SplitAxis, SplitRegion, TabDragMode, TabDragState, TabSplitTarget,
    TRASH_LOCATION_LABEL,
};

const TAB_BAR_REVEAL_DURATION: Duration = Duration::from_millis(180);
const TAB_BAR_HIDE_DURATION: Duration = Duration::from_millis(140);
const TAB_INTRO_DURATION: Duration = Duration::from_millis(180);
const TAB_CLOSE_DURATION: Duration = Duration::from_millis(150);
const TAB_REORDER_DURATION: Duration = Duration::from_millis(160);
const TAB_REORDER_HORIZONTAL_PADDING: f32 = 36.0;
const TAB_REORDER_SPACING: f32 = 6.0;
const TAB_REORDER_MIN_SLOT_WIDTH: f32 = 48.0;

#[derive(Debug, Clone, Copy)]
pub(super) enum TabBarReveal {
    Hidden,
    Opening {
        started_at: Instant,
        initial_fraction: f32,
    },
    Visible,
    Closing {
        started_at: Instant,
        initial_fraction: f32,
    },
}

impl Default for TabBarReveal {
    fn default() -> Self {
        Self::Hidden
    }
}

impl TabBarReveal {
    fn is_animating(self) -> bool {
        matches!(self, Self::Opening { .. } | Self::Closing { .. })
    }

    fn fraction(self) -> f32 {
        match self {
            Self::Hidden => 0.0,
            Self::Visible => 1.0,
            Self::Opening {
                started_at,
                initial_fraction,
            } => {
                let progress = animation_progress(started_at, TAB_BAR_REVEAL_DURATION);
                initial_fraction + (1.0 - initial_fraction) * ease_out_cubic(progress)
            }
            Self::Closing {
                started_at,
                initial_fraction,
            } => {
                let progress = animation_progress(started_at, TAB_BAR_HIDE_DURATION);
                initial_fraction * (1.0 - ease_out_cubic(progress))
            }
        }
        .clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TabAnimationState {
    intro_started_at: Option<Instant>,
    close: Option<TabCloseAnimation>,
    shift: Option<TabShiftAnimation>,
}

impl TabAnimationState {
    pub(crate) fn width_fraction(self) -> f32 {
        if let Some(close) = self.close {
            let progress = ease_out_cubic(animation_progress(close.started_at, TAB_CLOSE_DURATION));
            return (close.initial_fraction * (1.0 - progress)).clamp(0.0, 1.0);
        }

        self.intro_fraction()
    }

    fn intro_fraction(self) -> f32 {
        let Some(started_at) = self.intro_started_at else {
            return 1.0;
        };
        ease_out_cubic(animation_progress(started_at, TAB_INTRO_DURATION))
    }

    pub(crate) fn shift_offset(self) -> f32 {
        let Some(shift) = self.shift else {
            return 0.0;
        };
        let progress = ease_out_cubic(animation_progress(shift.started_at, TAB_REORDER_DURATION));
        shift.initial_offset * (1.0 - progress)
    }

    fn is_animating(self) -> bool {
        self.intro_started_at
            .is_some_and(|started_at| animation_progress(started_at, TAB_INTRO_DURATION) < 1.0)
            || self
                .close
                .is_some_and(|close| animation_progress(close.started_at, TAB_CLOSE_DURATION) < 1.0)
            || self.shift.is_some_and(|shift| {
                animation_progress(shift.started_at, TAB_REORDER_DURATION) < 1.0
            })
    }

    fn is_closing(self) -> bool {
        self.close.is_some()
    }

    fn close_is_finished(self) -> bool {
        self.close
            .is_some_and(|close| animation_progress(close.started_at, TAB_CLOSE_DURATION) >= 1.0)
    }
}

#[derive(Debug, Clone, Copy)]
struct TabCloseAnimation {
    started_at: Instant,
    initial_fraction: f32,
}

#[derive(Debug, Clone, Copy)]
struct TabShiftAnimation {
    started_at: Instant,
    initial_offset: f32,
}

pub(crate) struct TabDragPreview<'a> {
    pub(crate) directory: &'a Path,
    pub(crate) is_trash_view: bool,
}

impl FileBrowser {
    pub(crate) fn tab_bar_reveal_fraction(&self) -> f32 {
        self.tab_bar_reveal.fraction()
    }

    pub(super) fn tab_bar_reveal_animation_is_active(&self) -> bool {
        self.tab_bar_reveal.is_animating()
    }

    pub(super) fn advance_tab_bar_reveal_animation(&mut self) -> Task<Message> {
        match self.tab_bar_reveal {
            TabBarReveal::Opening { .. } if self.tab_bar_reveal_fraction() >= 1.0 => {
                self.tab_bar_reveal = TabBarReveal::Visible;
            }
            TabBarReveal::Closing { .. } if self.tab_bar_reveal_fraction() <= f32::EPSILON => {
                self.tab_bar_reveal = TabBarReveal::Hidden;
            }
            _ => {}
        }
        Task::none()
    }

    pub(super) fn tab_animation_is_active(&self) -> bool {
        !self.tab_animations.is_empty()
    }

    pub(super) fn advance_tab_animations(&mut self) -> Task<Message> {
        self.remove_closed_tabs_after_animation();
        self.prune_tab_animations();
        Task::none()
    }

    pub(crate) fn tab_drag_preview(&self) -> Option<TabDragPreview<'_>> {
        let drag = self.tab_drag.as_ref()?;
        if !drag.is_dragging() {
            return None;
        }

        let tab = if drag.source_pane_id == self.active_pane_id() {
            self.tabs.iter().find(|tab| tab.id == drag.tab_id)?
        } else {
            self.pane_by_id(drag.source_pane_id)?
                .tabs
                .iter()
                .find(|tab| tab.id == drag.tab_id)?
        };

        Some(TabDragPreview {
            directory: tab.directory.as_path(),
            is_trash_view: tab.is_trash_view,
        })
    }

    pub(super) fn sync_active_tab_state(&mut self) {
        let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == self.active_tab_id)
        else {
            return;
        };

        tab.directory = self.current_dir.clone();
        tab.is_trash_view = self.is_trash_view;
        tab.entries = self.entries.clone();
        tab.trash_entries = self.trash_entries.clone();
        tab.selected = self.selected.clone();
        tab.selected_paths = self.selected_paths.clone();
        tab.selection_anchor = self.selection_anchor.clone();
        tab.expanded_directories = self.expanded_directories.clone();
        tab.back_stack = self.back_stack.clone();
        tab.forward_stack = self.forward_stack.clone();
        self.sync_active_pane_state();
    }

    pub(super) fn open_directory_in_new_tab(&mut self, directory: PathBuf) -> Task<Message> {
        self.sync_active_tab_state();
        self.context_menu = None;
        self.clear_preview();
        self.is_column_view_settings_open = false;

        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs
            .push(BrowserTab::directory(tab_id, directory.clone()));
        self.start_tab_intro_animation(tab_id);

        self.sync_tab_bar_visibility();
        self.active_tab_id = tab_id;
        self.back_stack.clear();
        self.forward_stack.clear();
        self.navigate_to(directory, NavigationMode::KeepHistory)
    }

    pub(super) fn open_trash_in_new_tab(&mut self) -> Task<Message> {
        self.sync_active_tab_state();
        self.context_menu = None;
        self.clear_preview();
        self.is_column_view_settings_open = false;

        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(BrowserTab::trash(tab_id));
        self.start_tab_intro_animation(tab_id);

        self.sync_tab_bar_visibility();
        self.active_tab_id = tab_id;
        self.back_stack.clear();
        self.forward_stack.clear();
        self.open_trash_view(NavigationMode::KeepHistory)
    }

    pub(super) fn select_tab(&mut self, tab_id: usize) -> Task<Message> {
        if self.tab_is_closing(tab_id) {
            return Task::none();
        }

        if tab_id == self.active_tab_id {
            self.sync_active_tab_state();
            return Task::none();
        }

        let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id).cloned() else {
            return Task::none();
        };

        self.sync_active_tab_state();
        self.active_tab_id = tab.id;
        self.is_trash_view = tab.is_trash_view;
        self.entries = tab.entries;
        self.trash_entries = tab.trash_entries;
        self.selected = tab.selected;
        self.selected_paths = tab.selected_paths;
        self.selection_anchor = tab.selection_anchor;
        self.expanded_directories = tab.expanded_directories;
        self.back_stack = tab.back_stack;
        self.forward_stack = tab.forward_stack;
        self.current_dir = tab.directory;
        self.reload_current()
    }

    pub(super) fn close_tab(&mut self, tab_id: usize) -> Task<Message> {
        if self.open_tab_count() <= 1 || self.tab_is_closing(tab_id) {
            return Task::none();
        }

        let Some(closing_index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return Task::none();
        };
        let was_active = tab_id == self.active_tab_id;

        if self
            .tab_drag
            .as_ref()
            .is_some_and(|drag| drag.tab_id == tab_id)
        {
            self.finish_tab_drag();
        }

        self.start_tab_close_animation(tab_id);

        if !was_active {
            return Task::none();
        }

        let Some(adjacent_tab_id) = self.adjacent_open_tab_id(closing_index, tab_id) else {
            return Task::none();
        };
        self.select_tab(adjacent_tab_id)
    }

    pub(super) fn start_tab_drag(&mut self, pane_id: BrowserPaneId, tab_id: usize) {
        if pane_id != self.active_pane_id()
            || self.tab_is_closing(tab_id)
            || !self.tabs.iter().any(|tab| tab.id == tab_id)
        {
            return;
        }

        let mode = if self.keyboard_modifiers.control() && self.keyboard_modifiers.shift() {
            TabDragMode::Split
        } else {
            TabDragMode::Reorder
        };
        self.tab_drag = Some(TabDragState {
            source_pane_id: pane_id,
            tab_id,
            phase: FileDragPhase::WaitingForMovement {
                origin: self.cursor_position,
            },
            mode,
            split_target: None,
        });
    }

    pub(super) fn reorder_dragged_tab(
        &mut self,
        entered_pane_id: BrowserPaneId,
        entered_tab_id: usize,
    ) {
        let Some(drag) = self.tab_drag.as_ref() else {
            return;
        };
        if drag.mode != TabDragMode::Reorder
            || drag.source_pane_id != entered_pane_id
            || !drag.is_dragging()
        {
            return;
        }
        let dragged_tab_id = drag.tab_id;
        if dragged_tab_id == entered_tab_id
            || self.tab_is_closing(dragged_tab_id)
            || self.tab_is_closing(entered_tab_id)
        {
            return;
        }

        let Some(dragged_index) = self.tabs.iter().position(|tab| tab.id == dragged_tab_id) else {
            self.finish_tab_drag();
            return;
        };
        let Some(entered_index) = self.tabs.iter().position(|tab| tab.id == entered_tab_id) else {
            return;
        };

        let shifted_tab_ids = self.shifted_tab_ids_for_reorder(dragged_index, entered_index);
        let shift_offset = if dragged_index < entered_index {
            self.tab_reorder_slot_width()
        } else {
            -self.tab_reorder_slot_width()
        };

        let dragged_tab = self.tabs.remove(dragged_index);
        self.tabs.insert(entered_index, dragged_tab);
        self.start_tab_reorder_animations(shifted_tab_ids, shift_offset);
        self.sync_active_pane_state();
    }

    pub(super) fn finish_tab_drag(&mut self) {
        let Some(drag) = self.tab_drag.take() else {
            return;
        };
        if drag.mode != TabDragMode::Split || !drag.is_dragging() {
            return;
        }
        let Some(target) = drag.split_target else {
            return;
        };
        self.finish_tab_split_drag(drag.source_pane_id, drag.tab_id, target.region);
    }

    pub(super) fn update_tab_drag(&mut self, position: iced::Point) {
        let mut should_update_split_target = false;
        if let Some(drag) = &mut self.tab_drag {
            if let FileDragPhase::WaitingForMovement { origin } = drag.phase {
                let delta_x = position.x - origin.x;
                let delta_y = position.y - origin.y;
                if delta_x * delta_x + delta_y * delta_y
                    >= super::POINTER_DRAG_ACTIVATION_DISTANCE
                        * super::POINTER_DRAG_ACTIVATION_DISTANCE
                {
                    drag.phase = FileDragPhase::Dragging;
                }
            }
            should_update_split_target = drag.mode == TabDragMode::Split && drag.is_dragging();
        }

        if !should_update_split_target {
            return;
        }
        let split_target = self
            .split_region_at(position)
            .map(|region| TabSplitTarget { region });
        if let Some(drag) = &mut self.tab_drag {
            drag.split_target = split_target;
        }
    }

    fn finish_tab_split_drag(
        &mut self,
        source_pane_id: BrowserPaneId,
        tab_id: usize,
        region: SplitRegion,
    ) {
        self.sync_active_tab_state();

        match self.pane_layout {
            BrowserPaneLayout::Single { .. } => {
                self.split_single_pane_with_tab(source_pane_id, tab_id, region)
            }
            BrowserPaneLayout::Split { first, second, .. } => {
                self.move_tab_to_split_side(source_pane_id, tab_id, region, first, second)
            }
        }
    }

    fn split_single_pane_with_tab(
        &mut self,
        source_pane_id: BrowserPaneId,
        tab_id: usize,
        region: SplitRegion,
    ) {
        let destination_id = self.new_pane_id();
        let Some(tab) = self.detach_or_copy_tab(source_pane_id, tab_id) else {
            return;
        };
        let destination = pane_from_tab(destination_id, tab);
        self.panes.push(destination.clone());

        let (first, second) = if region.places_dragged_first() {
            (destination_id, source_pane_id)
        } else {
            (source_pane_id, destination_id)
        };
        self.pane_layout = BrowserPaneLayout::Split {
            axis: region.axis(),
            first,
            second,
            active: destination_id,
        };
        self.restore_pane_snapshot(destination);
    }

    fn move_tab_to_split_side(
        &mut self,
        source_pane_id: BrowserPaneId,
        tab_id: usize,
        region: SplitRegion,
        first: BrowserPaneId,
        second: BrowserPaneId,
    ) {
        let current_destination = if region.places_dragged_first() {
            first
        } else {
            second
        };
        let destination_id = if current_destination == source_pane_id {
            source_pane_id
        } else {
            current_destination
        };

        if destination_id == source_pane_id {
            if let Some(source) = self.pane_by_id_mut(source_pane_id) {
                if source.tabs.iter().any(|tab| tab.id == tab_id) {
                    source.active_tab_id = tab_id;
                    apply_active_tab_to_pane(source);
                }
            }
        } else {
            let Some(tab) = self.detach_or_copy_tab(source_pane_id, tab_id) else {
                return;
            };
            if let Some(destination) = self.pane_by_id_mut(destination_id) {
                destination.tabs.push(tab.clone());
                destination.active_tab_id = tab.id;
                apply_tab_to_pane(destination, &tab);
            }
        }

        let other_id = if destination_id == first {
            second
        } else {
            first
        };
        let (layout_first, layout_second) = if region.places_dragged_first() {
            (destination_id, other_id)
        } else {
            (other_id, destination_id)
        };
        self.pane_layout = BrowserPaneLayout::Split {
            axis: region.axis(),
            first: layout_first,
            second: layout_second,
            active: destination_id,
        };

        if let Some(destination) = self.pane_by_id(destination_id).cloned() {
            self.restore_pane_snapshot(destination);
        }
    }

    fn detach_or_copy_tab(
        &mut self,
        source_pane_id: BrowserPaneId,
        tab_id: usize,
    ) -> Option<BrowserTab> {
        let copied_tab = {
            let source = self.pane_by_id_mut(source_pane_id)?;
            let tab_index = source.tabs.iter().position(|tab| tab.id == tab_id)?;
            if source.tabs.len() == 1 {
                Some(source.tabs[tab_index].clone())
            } else {
                let tab = source.tabs.remove(tab_index);
                if source.active_tab_id == tab_id {
                    let adjacent_index = tab_index.min(source.tabs.len() - 1);
                    source.active_tab_id = source.tabs[adjacent_index].id;
                    apply_active_tab_to_pane(source);
                }
                return Some(tab);
            }
        };

        let mut cloned = copied_tab?;
        cloned.id = self.next_tab_id;
        self.next_tab_id += 1;
        Some(cloned)
    }

    fn sync_tab_bar_visibility(&mut self) {
        if self.tabs.len() > 1 {
            self.reveal_tab_bar();
        } else {
            self.hide_tab_bar();
        }
    }

    fn reveal_tab_bar(&mut self) {
        if matches!(
            self.tab_bar_reveal,
            TabBarReveal::Visible | TabBarReveal::Opening { .. }
        ) {
            return;
        }

        let initial_fraction = self.tab_bar_reveal_fraction();
        self.tab_bar_reveal = if (1.0 - initial_fraction) <= f32::EPSILON {
            TabBarReveal::Visible
        } else {
            TabBarReveal::Opening {
                started_at: Instant::now(),
                initial_fraction,
            }
        };
    }

    fn hide_tab_bar(&mut self) {
        if matches!(
            self.tab_bar_reveal,
            TabBarReveal::Hidden | TabBarReveal::Closing { .. }
        ) {
            return;
        }

        let initial_fraction = self.tab_bar_reveal_fraction();
        self.tab_bar_reveal = if initial_fraction <= f32::EPSILON {
            TabBarReveal::Hidden
        } else {
            TabBarReveal::Closing {
                started_at: Instant::now(),
                initial_fraction,
            }
        };
    }

    fn start_tab_intro_animation(&mut self, tab_id: usize) {
        self.tab_animations
            .entry(tab_id)
            .or_default()
            .intro_started_at = Some(Instant::now());
    }

    fn start_tab_close_animation(&mut self, tab_id: usize) {
        let initial_fraction = self
            .tab_animations
            .get(&tab_id)
            .map(|animation| animation.width_fraction())
            .unwrap_or(1.0);
        self.tab_animations.entry(tab_id).or_default().close = Some(TabCloseAnimation {
            started_at: Instant::now(),
            initial_fraction,
        });
    }

    fn start_tab_reorder_animations(&mut self, tab_ids: Vec<usize>, shift_offset: f32) {
        let started_at = Instant::now();
        for tab_id in tab_ids {
            let current_offset = self
                .tab_animations
                .get(&tab_id)
                .map(|animation| animation.shift_offset())
                .unwrap_or(0.0);
            self.tab_animations.entry(tab_id).or_default().shift = Some(TabShiftAnimation {
                started_at,
                initial_offset: current_offset + shift_offset,
            });
        }
    }

    fn remove_closed_tabs_after_animation(&mut self) {
        let closed_tab_ids = self
            .tab_animations
            .iter()
            .filter_map(|(tab_id, animation)| animation.close_is_finished().then_some(*tab_id))
            .collect::<Vec<_>>();
        if closed_tab_ids.is_empty() {
            return;
        }

        for tab_id in closed_tab_ids {
            if let Some(tab_index) = self.tabs.iter().position(|tab| tab.id == tab_id) {
                self.tabs.remove(tab_index);
            }
            self.tab_animations.remove(&tab_id);
            if self
                .tab_drag
                .as_ref()
                .is_some_and(|drag| drag.tab_id == tab_id)
            {
                self.finish_tab_drag();
            }
        }

        self.sync_tab_bar_visibility();
        self.sync_active_pane_state();
    }

    fn shifted_tab_ids_for_reorder(
        &self,
        dragged_index: usize,
        entered_index: usize,
    ) -> Vec<usize> {
        if dragged_index < entered_index {
            self.tabs[dragged_index + 1..=entered_index]
                .iter()
                .map(|tab| tab.id)
                .collect()
        } else {
            self.tabs[entered_index..dragged_index]
                .iter()
                .map(|tab| tab.id)
                .collect()
        }
    }

    fn tab_is_closing(&self, tab_id: usize) -> bool {
        self.tab_animations
            .get(&tab_id)
            .is_some_and(|animation| animation.is_closing())
    }

    fn open_tab_count(&self) -> usize {
        self.tabs
            .iter()
            .filter(|tab| !self.tab_is_closing(tab.id))
            .count()
    }

    fn adjacent_open_tab_id(&self, closing_index: usize, closing_tab_id: usize) -> Option<usize> {
        self.tabs[..closing_index]
            .iter()
            .rev()
            .chain(self.tabs[closing_index + 1..].iter())
            .find(|tab| tab.id != closing_tab_id && !self.tab_is_closing(tab.id))
            .map(|tab| tab.id)
    }

    fn prune_tab_animations(&mut self) {
        let visible_tab_ids = self.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
        self.tab_animations.retain(|tab_id, animation| {
            visible_tab_ids.contains(tab_id) && animation.is_animating()
        });
    }

    fn tab_reorder_slot_width(&self) -> f32 {
        let tab_count = self.tabs.len().max(1) as f32;
        let available_width = self.active_tab_bar_width_estimate()
            - TAB_REORDER_HORIZONTAL_PADDING
            - TAB_REORDER_SPACING * (tab_count - 1.0);
        (available_width / tab_count).max(TAB_REORDER_MIN_SLOT_WIDTH)
    }

    fn active_tab_bar_width_estimate(&self) -> f32 {
        let content_width = (self.main_window_width - self.sidebar_width).max(1.0);
        match self.pane_layout {
            BrowserPaneLayout::Split {
                axis: SplitAxis::Horizontal,
                ..
            } => content_width / 2.0,
            BrowserPaneLayout::Single { .. } | BrowserPaneLayout::Split { .. } => content_width,
        }
    }
}

fn pane_from_tab(pane_id: BrowserPaneId, tab: BrowserTab) -> BrowserPane {
    let mut pane = BrowserPane {
        id: pane_id,
        current_dir: tab.directory.clone(),
        is_trash_view: tab.is_trash_view,
        entries: tab.entries.clone(),
        trash_entries: tab.trash_entries.clone(),
        selected: tab.selected.clone(),
        selected_paths: tab.selected_paths.clone(),
        selection_anchor: tab.selection_anchor.clone(),
        expanded_directories: tab.expanded_directories.clone(),
        column_viewports: HashMap::new(),
        tabs: vec![tab.clone()],
        active_tab_id: tab.id,
        path_input: path_input_for_tab(&tab),
        path_suggestions: Vec::new(),
        path_suggestion_selection: None,
        path_suggestion_generation: 0,
        back_stack: tab.back_stack.clone(),
        forward_stack: tab.forward_stack.clone(),
        is_loading: false,
    };
    pane.sync_active_tab_state();
    pane
}

pub(super) fn apply_active_tab_to_pane(pane: &mut BrowserPane) {
    let Some(tab) = pane
        .tabs
        .iter()
        .find(|tab| tab.id == pane.active_tab_id)
        .cloned()
    else {
        return;
    };
    apply_tab_to_pane(pane, &tab);
}

pub(super) fn apply_tab_to_pane(pane: &mut BrowserPane, tab: &BrowserTab) {
    pane.current_dir = tab.directory.clone();
    pane.is_trash_view = tab.is_trash_view;
    pane.entries = tab.entries.clone();
    pane.trash_entries = tab.trash_entries.clone();
    pane.selected = tab.selected.clone();
    pane.selected_paths = tab.selected_paths.clone();
    pane.selection_anchor = tab.selection_anchor.clone();
    pane.expanded_directories = tab.expanded_directories.clone();
    pane.path_input = path_input_for_tab(tab);
    pane.path_suggestions.clear();
    pane.path_suggestion_selection = None;
    pane.back_stack = tab.back_stack.clone();
    pane.forward_stack = tab.forward_stack.clone();
    pane.is_loading = false;
    pane.sync_active_tab_state();
}

fn path_input_for_tab(tab: &BrowserTab) -> String {
    if tab.is_trash_view {
        TRASH_LOCATION_LABEL.to_owned()
    } else {
        path_text(&tab.directory)
    }
}

fn animation_progress(started_at: Instant, duration: Duration) -> f32 {
    (started_at.elapsed().as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
}

fn ease_out_cubic(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    1.0 - (1.0 - progress).powi(3)
}
