use std::path::Path;

use iced::advanced::widget as advanced_widget;
use iced::advanced::widget::operation::{Operation, Scrollable as ScrollableOperation};
use iced::widget::scrollable;
use iced::{mouse, Rectangle, Task, Vector};

use super::{FileBrowser, COLUMN_BROWSER_WHEEL_LINE_PIXELS};
use crate::model::Message;
use crate::three_column_view::{
    column_directories, COLUMN_RESIZE_DIVIDER_WIDTH, DEFAULT_VISIBLE_COLUMN_COUNT,
};
use crate::view::column_browser_scroll_id;

struct ColumnBrowserScrollBy {
    target: advanced_widget::Id,
    delta_x: f32,
}

struct ColumnBrowserScrollToOffset {
    target: advanced_widget::Id,
    offset_x: f32,
}

impl ColumnBrowserScrollBy {
    fn new(target: iced::widget::Id, delta_x: f32) -> Self {
        Self {
            target: target.into(),
            delta_x,
        }
    }
}

impl ColumnBrowserScrollToOffset {
    fn new(target: iced::widget::Id, offset_x: f32) -> Self {
        Self {
            target: target.into(),
            offset_x,
        }
    }
}

impl Operation<Message> for ColumnBrowserScrollBy {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Message>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&advanced_widget::Id>,
        _bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn ScrollableOperation,
    ) {
        if id == Some(&self.target) {
            state.scroll_to(scrollable::AbsoluteOffset {
                x: Some((translation.x - self.delta_x).max(0.0)),
                y: Some(translation.y.max(0.0)),
            });
        }
    }
}

impl Operation<Message> for ColumnBrowserScrollToOffset {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Message>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&advanced_widget::Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn ScrollableOperation,
    ) {
        if id == Some(&self.target) {
            let max_offset_x = (content_bounds.width - bounds.width).max(0.0);
            state.scroll_to(scrollable::AbsoluteOffset {
                x: Some(self.offset_x.clamp(0.0, max_offset_x)),
                y: Some(translation.y.max(0.0)),
            });
        }
    }
}

impl FileBrowser {
    pub(super) fn focus_latest_column(&self) -> Task<Message> {
        iced::widget::operation::snap_to(
            column_browser_scroll_id(self.active_pane_id()),
            scrollable::RelativeOffset { x: 1.0, y: 0.0 },
        )
    }

    pub(super) fn focus_column_containing_path(&self, path: &Path) -> Task<Message> {
        let Some(column_index) = self.column_index_containing_path(path) else {
            return Task::none();
        };
        let start_column_index = focused_column_scroll_start_index(column_index);
        let offset_x = self.column_scroll_offset(start_column_index);
        advanced_widget::operate(ColumnBrowserScrollToOffset::new(
            column_browser_scroll_id(self.active_pane_id()),
            offset_x,
        ))
    }

    pub(super) fn handle_column_browser_wheel_scrolled(
        &self,
        delta: mouse::ScrollDelta,
    ) -> Option<Task<Message>> {
        if !self.is_cursor_over_column_browser {
            return None;
        }

        let Some(delta_x) = self.column_browser_horizontal_wheel_delta(delta) else {
            return None;
        };

        Some(advanced_widget::operate(ColumnBrowserScrollBy::new(
            column_browser_scroll_id(self.active_pane_id()),
            delta_x,
        )))
    }

    fn column_browser_horizontal_wheel_delta(&self, delta: mouse::ScrollDelta) -> Option<f32> {
        let delta_x = match delta {
            mouse::ScrollDelta::Lines { x, y } => {
                let horizontal_lines = if self.keyboard_modifiers.shift() {
                    y
                } else {
                    x
                };
                horizontal_lines * COLUMN_BROWSER_WHEEL_LINE_PIXELS
            }
            mouse::ScrollDelta::Pixels { x, y } => {
                if self.keyboard_modifiers.shift() {
                    y
                } else {
                    x
                }
            }
        };

        (delta_x.abs() > f32::EPSILON).then_some(delta_x)
    }

    fn column_index_containing_path(&self, path: &Path) -> Option<usize> {
        let directory = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.current_dir.clone());
        column_directories(self)
            .iter()
            .position(|column_directory| column_directory == &directory)
    }

    fn column_scroll_offset(&self, start_column_index: usize) -> f32 {
        (0..start_column_index)
            .map(|column_index| self.column_width(column_index) + COLUMN_RESIZE_DIVIDER_WIDTH)
            .sum()
    }
}

fn focused_column_scroll_start_index(column_index: usize) -> usize {
    let focused_slot = DEFAULT_VISIBLE_COLUMN_COUNT.saturating_sub(1) / 2;
    column_index.saturating_sub(focused_slot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_column_scroll_start_keeps_first_column_at_origin() {
        assert_eq!(focused_column_scroll_start_index(0), 0);
        assert_eq!(focused_column_scroll_start_index(1), 0);
    }

    #[test]
    fn focused_column_scroll_start_centers_deeper_columns() {
        assert_eq!(focused_column_scroll_start_index(2), 1);
        assert_eq!(focused_column_scroll_start_index(4), 3);
    }
}
