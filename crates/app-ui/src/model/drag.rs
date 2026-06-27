use std::path::PathBuf;
use std::time::Instant;

use iced::Point;

use super::SplitRegion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabDragMode {
    Reorder,
    Split,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabSplitTarget {
    pub(crate) region: SplitRegion,
}

#[derive(Debug, Clone)]
pub(crate) struct TabDragState {
    pub(crate) source_pane_id: super::BrowserPaneId,
    pub(crate) tab_id: usize,
    pub(crate) phase: FileDragPhase,
    pub(crate) mode: TabDragMode,
    pub(crate) split_target: Option<TabSplitTarget>,
}

impl TabDragState {
    pub(crate) fn is_dragging(&self) -> bool {
        matches!(self.phase, FileDragPhase::Dragging)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneDropTarget {
    Split(SplitRegion),
    Merge(super::BrowserPaneId),
}

#[derive(Debug, Clone)]
pub(crate) struct PaneDragState {
    pub(crate) source_pane_id: super::BrowserPaneId,
    pub(crate) phase: FileDragPhase,
    pub(crate) target: Option<PaneDropTarget>,
}

impl PaneDragState {
    pub(crate) fn is_dragging(&self) -> bool {
        matches!(self.phase, FileDragPhase::Dragging)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FileDragState {
    pub(crate) sources: Vec<PathBuf>,
    pub(crate) pressed_path: PathBuf,
    pub(crate) target: Option<FileDragTarget>,
    pub(crate) phase: FileDragPhase,
    pub(crate) native_dnd: FileDragNativeDndState,
    pub(crate) column_directories_snapshot: Vec<PathBuf>,
}

impl FileDragState {
    pub(crate) fn is_dragging(&self) -> bool {
        matches!(self.phase, FileDragPhase::Dragging)
    }

    pub(crate) fn can_request_wayland_dnd(&self) -> bool {
        self.native_dnd == FileDragNativeDndState::NotRequested
            && matches!(
                self.phase,
                FileDragPhase::WaitingForMovement { .. } | FileDragPhase::Dragging
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileDragTarget {
    Directory(PathBuf),
    SidebarBookmarkSlot(SidebarBookmarkDropSlot),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileDragNativeDndState {
    NotRequested,
    WaylandRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarBookmarkDropSlot {
    Insert { index: usize },
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarBookmarkDragState {
    pub(crate) path: PathBuf,
    pub(crate) origin: Point,
    pub(crate) source_index: usize,
    pub(crate) phase: FileDragPhase,
    pub(crate) order_changed: bool,
}

impl SidebarBookmarkDragState {
    pub(crate) fn is_dragging(&self) -> bool {
        matches!(self.phase, FileDragPhase::Dragging)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileDragPhase {
    WaitingForMovement { origin: Point },
    Dragging,
}

#[derive(Debug, Clone)]
pub(crate) struct LastActivationClick {
    pub(crate) path: PathBuf,
    pub(crate) at: Instant,
}
