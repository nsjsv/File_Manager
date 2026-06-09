use std::path::Path;

use iced::{event, Point};

use super::tabs::apply_active_tab_to_pane;
use super::{FileBrowser, POINTER_DRAG_ACTIVATION_DISTANCE};
use crate::app::panes::SplitOverlayBounds;
use crate::model::{
    BrowserPane, BrowserPaneId, BrowserPaneLayout, FileDragPhase, OperationQueuePanelMode,
    PaneDragState, PaneDropTarget, SplitAxis, SplitRegion,
};
use crate::sidebar::SIDEBAR_WIDTH;

const PANE_DROP_CENTER_FRACTION: f32 = 0.28;

#[derive(Debug, Clone, Copy)]
struct PaneBounds {
    id: BrowserPaneId,
    top_left: Point,
    width: f32,
    height: f32,
}

pub(crate) struct PaneDragPreview<'a> {
    pub(crate) directory: &'a Path,
    pub(crate) is_trash_view: bool,
}

impl FileBrowser {
    pub(super) fn ctrl_shift_pane_drag_shortcut_is_pressed(&self) -> bool {
        self.keyboard_modifiers.control()
            && self.keyboard_modifiers.shift()
            && matches!(self.pane_layout, BrowserPaneLayout::Split { .. })
    }

    pub(super) fn start_ctrl_shift_pane_drag(&mut self, status: event::Status) -> bool {
        if !self.ctrl_shift_pane_drag_shortcut_is_pressed() {
            return false;
        }
        if !self.pane_drag_can_start(status) {
            return false;
        }

        let Some(source_pane_id) = self.hovered_pane_id else {
            return false;
        };
        if self.pane_by_id(source_pane_id).is_none() {
            return false;
        }

        self.sync_active_tab_state();
        self.pane_drag = Some(PaneDragState {
            source_pane_id,
            phase: FileDragPhase::WaitingForMovement {
                origin: self.cursor_position,
            },
            target: None,
        });
        true
    }

    pub(super) fn update_pane_drag(&mut self, position: Point) {
        let mut source_pane_id = None;
        if let Some(drag) = &mut self.pane_drag {
            if let FileDragPhase::WaitingForMovement { origin } = drag.phase {
                let delta_x = position.x - origin.x;
                let delta_y = position.y - origin.y;
                if delta_x * delta_x + delta_y * delta_y
                    >= POINTER_DRAG_ACTIVATION_DISTANCE * POINTER_DRAG_ACTIVATION_DISTANCE
                {
                    drag.phase = FileDragPhase::Dragging;
                }
            }

            if drag.is_dragging() {
                source_pane_id = Some(drag.source_pane_id);
            }
        }

        let Some(source_pane_id) = source_pane_id else {
            return;
        };
        let target = self.pane_drop_target_at(source_pane_id, position);
        if let Some(drag) = &mut self.pane_drag {
            drag.target = target;
        }
    }

    pub(super) fn finish_pane_drag(&mut self) {
        let Some(drag) = self.pane_drag.take() else {
            return;
        };
        if !drag.is_dragging() {
            return;
        }

        match drag.target {
            Some(PaneDropTarget::Merge(target_pane_id)) => {
                self.merge_dragged_pane_into_target(drag.source_pane_id, target_pane_id);
            }
            Some(PaneDropTarget::Split(region)) => {
                self.move_dragged_pane_to_split_region(drag.source_pane_id, region);
            }
            None => {}
        }
    }

    pub(crate) fn pane_drag_preview(&self) -> Option<PaneDragPreview<'_>> {
        let drag = self.pane_drag.as_ref()?;
        if !drag.is_dragging() {
            return None;
        }

        let pane = self.pane_by_id(drag.source_pane_id)?;
        let tab = pane.tabs.iter().find(|tab| tab.id == pane.active_tab_id)?;
        Some(PaneDragPreview {
            directory: tab.directory.as_path(),
            is_trash_view: tab.is_trash_view,
        })
    }

    pub(crate) fn pane_drag_overlay_bounds(&self) -> Option<SplitOverlayBounds> {
        let drag = self.pane_drag.as_ref()?;
        if !drag.is_dragging() {
            return None;
        }

        match drag.target? {
            PaneDropTarget::Split(region) => Some(self.split_region_overlay_bounds(region)),
            PaneDropTarget::Merge(target_pane_id) => {
                self.pane_bounds_by_id(target_pane_id)
                    .map(|bounds| SplitOverlayBounds {
                        top_left: bounds.top_left,
                        width: bounds.width,
                        height: bounds.height,
                    })
            }
        }
    }

    fn pane_drag_can_start(&self, status: event::Status) -> bool {
        let pointer_status_allows_start = match status {
            event::Status::Ignored => true,
            event::Status::Captured => {
                self.is_cursor_over_column_browser && self.renaming.is_none()
            }
        };
        pointer_status_allows_start
            && self.destructive_action_confirmation.is_none()
            && self.transfer_conflict.is_none()
            && self.context_menu.is_none()
            && !self.is_column_view_settings_open
            && self.path_suggestions.is_empty()
            && !self.operation_queue_interaction_is_open()
            && self.tab_drag.is_none()
            && self.file_drag.is_none()
            && self.sidebar_bookmark_drag.is_none()
            && self.column_resize_drag.is_none()
            && self.selection_marquee.is_none()
    }

    fn operation_queue_interaction_is_open(&self) -> bool {
        self.operation_queue.is_panel_open()
            && self.operation_queue_panel_mode == OperationQueuePanelMode::InteractiveList
    }

    fn pane_drop_target_at(
        &self,
        source_pane_id: BrowserPaneId,
        position: Point,
    ) -> Option<PaneDropTarget> {
        let bounds = self.pane_bounds_at(position)?;
        if bounds.id == source_pane_id {
            return None;
        }

        Some(bounds.drop_target_at(position))
    }

    fn move_dragged_pane_to_split_region(
        &mut self,
        source_pane_id: BrowserPaneId,
        region: SplitRegion,
    ) {
        let BrowserPaneLayout::Split { first, second, .. } = self.pane_layout else {
            return;
        };
        let Some(other_pane_id) = other_split_pane_id(source_pane_id, first, second) else {
            return;
        };

        self.sync_active_tab_state();
        let (first, second) = if region.places_dragged_first() {
            (source_pane_id, other_pane_id)
        } else {
            (other_pane_id, source_pane_id)
        };
        self.pane_layout = BrowserPaneLayout::Split {
            axis: region.axis(),
            first,
            second,
            active: source_pane_id,
        };

        if let Some(source) = self.pane_by_id(source_pane_id).cloned() {
            self.restore_pane_snapshot(source);
        }
    }

    fn merge_dragged_pane_into_target(
        &mut self,
        source_pane_id: BrowserPaneId,
        target_pane_id: BrowserPaneId,
    ) {
        if source_pane_id == target_pane_id {
            return;
        }

        self.sync_active_tab_state();
        let Some(source) = self.pane_by_id(source_pane_id).cloned() else {
            return;
        };
        let Some(mut target) = self.pane_by_id(target_pane_id).cloned() else {
            return;
        };

        merge_pane_tabs(&mut target, source);
        self.panes.retain(|pane| pane.id != source_pane_id);
        if let Some(target_slot) = self.pane_by_id_mut(target_pane_id) {
            *target_slot = target.clone();
        } else {
            self.panes.push(target.clone());
        }

        self.pane_layout = BrowserPaneLayout::Single {
            active: target_pane_id,
        };
        self.restore_pane_snapshot(target);
    }

    fn pane_bounds_at(&self, position: Point) -> Option<PaneBounds> {
        self.pane_bounds()
            .into_iter()
            .find(|bounds| bounds.contains(position))
    }

    fn pane_bounds_by_id(&self, pane_id: BrowserPaneId) -> Option<PaneBounds> {
        self.pane_bounds()
            .into_iter()
            .find(|bounds| bounds.id == pane_id)
    }

    fn pane_bounds(&self) -> Vec<PaneBounds> {
        let content_width = (self.main_window_width - SIDEBAR_WIDTH).max(1.0);
        let content_height = self.main_window_height.max(1.0);
        match self.pane_layout {
            BrowserPaneLayout::Single { active } => vec![PaneBounds {
                id: active,
                top_left: Point::new(SIDEBAR_WIDTH, 0.0),
                width: content_width,
                height: content_height,
            }],
            BrowserPaneLayout::Split {
                axis: SplitAxis::Horizontal,
                first,
                second,
                ..
            } => {
                let half_width = content_width / 2.0;
                vec![
                    PaneBounds {
                        id: first,
                        top_left: Point::new(SIDEBAR_WIDTH, 0.0),
                        width: half_width,
                        height: content_height,
                    },
                    PaneBounds {
                        id: second,
                        top_left: Point::new(SIDEBAR_WIDTH + half_width, 0.0),
                        width: half_width,
                        height: content_height,
                    },
                ]
            }
            BrowserPaneLayout::Split {
                axis: SplitAxis::Vertical,
                first,
                second,
                ..
            } => {
                let half_height = content_height / 2.0;
                vec![
                    PaneBounds {
                        id: first,
                        top_left: Point::new(SIDEBAR_WIDTH, 0.0),
                        width: content_width,
                        height: half_height,
                    },
                    PaneBounds {
                        id: second,
                        top_left: Point::new(SIDEBAR_WIDTH, half_height),
                        width: content_width,
                        height: half_height,
                    },
                ]
            }
        }
    }

    fn split_region_overlay_bounds(&self, region: SplitRegion) -> SplitOverlayBounds {
        let content_width = (self.main_window_width - SIDEBAR_WIDTH).max(1.0);
        let content_height = self.main_window_height.max(1.0);
        let half_width = content_width / 2.0;
        let half_height = content_height / 2.0;

        match region {
            SplitRegion::Left => SplitOverlayBounds {
                top_left: Point::new(SIDEBAR_WIDTH, 0.0),
                width: half_width,
                height: content_height,
            },
            SplitRegion::Right => SplitOverlayBounds {
                top_left: Point::new(SIDEBAR_WIDTH + half_width, 0.0),
                width: half_width,
                height: content_height,
            },
            SplitRegion::Top => SplitOverlayBounds {
                top_left: Point::new(SIDEBAR_WIDTH, 0.0),
                width: content_width,
                height: half_height,
            },
            SplitRegion::Bottom => SplitOverlayBounds {
                top_left: Point::new(SIDEBAR_WIDTH, half_height),
                width: content_width,
                height: half_height,
            },
        }
    }
}

impl PaneBounds {
    fn contains(self, position: Point) -> bool {
        position.x >= self.top_left.x
            && position.x <= self.top_left.x + self.width
            && position.y >= self.top_left.y
            && position.y <= self.top_left.y + self.height
    }

    fn drop_target_at(self, position: Point) -> PaneDropTarget {
        let local_x = ((position.x - self.top_left.x) / self.width).clamp(0.0, 1.0);
        let local_y = ((position.y - self.top_left.y) / self.height).clamp(0.0, 1.0);
        let center_min = PANE_DROP_CENTER_FRACTION;
        let center_max = 1.0 - PANE_DROP_CENTER_FRACTION;
        if local_x >= center_min
            && local_x <= center_max
            && local_y >= center_min
            && local_y <= center_max
        {
            return PaneDropTarget::Merge(self.id);
        }

        PaneDropTarget::Split(closest_split_region(local_x, local_y))
    }
}

fn closest_split_region(local_x: f32, local_y: f32) -> SplitRegion {
    let mut closest_distance = local_x;
    let mut region = SplitRegion::Left;
    let right_distance = 1.0 - local_x;
    if right_distance < closest_distance {
        closest_distance = right_distance;
        region = SplitRegion::Right;
    }
    if local_y < closest_distance {
        closest_distance = local_y;
        region = SplitRegion::Top;
    }
    if 1.0 - local_y < closest_distance {
        region = SplitRegion::Bottom;
    }
    region
}

fn other_split_pane_id(
    source_pane_id: BrowserPaneId,
    first: BrowserPaneId,
    second: BrowserPaneId,
) -> Option<BrowserPaneId> {
    if source_pane_id == first {
        Some(second)
    } else if source_pane_id == second {
        Some(first)
    } else {
        None
    }
}

fn merge_pane_tabs(target: &mut BrowserPane, source: BrowserPane) {
    let source_active_tab_id = source.active_tab_id;
    target.tabs.extend(source.tabs);
    target.active_tab_id = source_active_tab_id;
    apply_active_tab_to_pane(target);
}
