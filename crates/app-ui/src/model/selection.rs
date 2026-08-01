use std::collections::HashSet;
use std::path::PathBuf;

use iced::{Point, Rectangle};

use super::BrowserPaneId;

#[derive(Debug, Clone)]
pub(crate) struct ColumnEntryBounds {
    pub(crate) pane_id: BrowserPaneId,
    pub(crate) path: PathBuf,
    pub(crate) bounds: Rectangle,
}

#[derive(Debug, Clone)]
pub(crate) struct SelectionMarquee {
    pub(crate) start: Point,
    pub(crate) current: Point,
    pub(crate) source: SelectionMarqueeSource,
    pub(crate) phase: SelectionMarqueePhase,
    pub(crate) base_selection: HashSet<PathBuf>,
    pub(crate) preserve_existing: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum SelectionMarqueeSource {
    PaneBlank,
    ColumnBlank { directory: PathBuf },
    IconGridPanel { directory: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionMarqueePhase {
    WaitingForMovement,
    Selecting,
}

impl SelectionMarquee {
    pub(crate) fn top_left(&self) -> Point {
        Point::new(
            self.start.x.min(self.current.x),
            self.start.y.min(self.current.y),
        )
    }

    pub(crate) fn width(&self) -> f32 {
        (self.current.x - self.start.x).abs().max(1.0)
    }

    pub(crate) fn height(&self) -> f32 {
        (self.current.y - self.start.y).abs().max(1.0)
    }

    pub(crate) fn rectangle(&self) -> Rectangle {
        let top_left = self.top_left();
        Rectangle {
            x: top_left.x,
            y: top_left.y,
            width: self.width(),
            height: self.height(),
        }
    }

    pub(crate) fn is_selecting(&self) -> bool {
        self.phase == SelectionMarqueePhase::Selecting
    }
}
