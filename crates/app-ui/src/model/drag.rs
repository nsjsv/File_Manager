use std::path::PathBuf;
use std::time::Instant;

use desktop_linux::WaylandFileDragSessionId;
use iced::{Point, Rectangle};

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

#[derive(Debug, Clone, Copy)]
pub(crate) struct PaneDragPointerPress {
    pub(crate) source_pane_id: super::BrowserPaneId,
    pub(crate) origin: Point,
}

#[derive(Debug, Clone)]
pub(crate) struct FileDragState {
    pub(crate) sources: Vec<PathBuf>,
    pub(crate) pressed_path: PathBuf,
    pub(crate) target: Option<FileDragTarget>,
    pub(crate) phase: FileDragPhase,
    pub(crate) native_dnd: FileDragNativeDndState,
    pub(crate) wayland_target: Option<WaylandFileDragTargetSnapshot>,
    pub(crate) column_directories_snapshot: Vec<PathBuf>,
}

impl FileDragState {
    pub(crate) fn is_dragging(&self) -> bool {
        matches!(self.phase, FileDragPhase::Dragging)
    }

    pub(crate) fn can_start_native_dnd(&self) -> bool {
        self.native_dnd == FileDragNativeDndState::NotRequested && self.is_dragging()
    }

    pub(crate) fn displays_iced_drag_preview(&self) -> bool {
        self.is_dragging()
            && matches!(
                self.native_dnd,
                FileDragNativeDndState::NotRequested
                    | FileDragNativeDndState::MeasuringTargets(_)
                    | FileDragNativeDndState::Requested(_)
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileDragTarget {
    Directory(PathBuf),
    SidebarBookmarkSlot(SidebarBookmarkDropSlot),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FileDragHitTestBounds {
    pub(crate) entries: Vec<super::ColumnEntryBounds>,
    pub(crate) breadcrumbs: Vec<BreadcrumbDropTargetBounds>,
    pub(crate) directory_targets: Vec<DirectoryFileDragTargetBounds>,
    pub(crate) sidebar_directories: Vec<SidebarFileDragTargetBounds>,
    pub(crate) empty_sidebar_bookmarks: Option<Rectangle>,
}

#[derive(Debug, Clone)]
pub(crate) struct DirectoryFileDragTargetBounds {
    pub(crate) pane_id: super::BrowserPaneId,
    pub(crate) directory: PathBuf,
    pub(crate) bounds: Rectangle,
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarFileDragTargetBounds {
    pub(crate) directory: PathBuf,
    pub(crate) favorite_index: Option<usize>,
    pub(crate) bounds: Rectangle,
}

#[derive(Debug, Clone)]
pub(crate) struct WaylandFileDragEntryTargetBounds {
    pub(crate) directory: PathBuf,
    pub(crate) path: PathBuf,
    pub(crate) bounds: Rectangle,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WaylandFileDragHitTestBounds {
    pub(crate) entries: Vec<WaylandFileDragEntryTargetBounds>,
    pub(crate) breadcrumbs: Vec<BreadcrumbDropTargetBounds>,
    pub(crate) directory_targets: Vec<DirectoryFileDragTargetBounds>,
    pub(crate) sidebar_directories: Vec<SidebarFileDragTargetBounds>,
    pub(crate) empty_sidebar_bookmarks: Option<Rectangle>,
}

#[derive(Debug, Clone)]
pub(crate) struct WaylandFileDragTargetSnapshot {
    pub(crate) session_id: WaylandFileDragSessionId,
    pub(crate) hit_test_bounds: WaylandFileDragHitTestBounds,
    pub(crate) bookmark_source: Option<PathBuf>,
    pub(crate) position: Option<Point>,
    pub(crate) target: Option<FileDragTarget>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BreadcrumbDropTargetBounds {
    pub(crate) pane_id: super::BrowserPaneId,
    pub(crate) directory: PathBuf,
    pub(crate) item_bounds: Rectangle,
    pub(crate) viewport_bounds: Rectangle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileDragNativeDndState {
    NotRequested,
    MeasuringTargets(u64),
    Requested(WaylandFileDragSessionId),
    Started(WaylandFileDragSessionId),
    Dropped(WaylandFileDragSessionId),
}

impl FileDragNativeDndState {
    pub(crate) fn session_id(self) -> Option<WaylandFileDragSessionId> {
        match self {
            Self::NotRequested | Self::MeasuringTargets(_) => None,
            Self::Requested(session_id) | Self::Started(session_id) | Self::Dropped(session_id) => {
                Some(session_id)
            }
        }
    }
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
