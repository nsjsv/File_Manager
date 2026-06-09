use iced::advanced::widget as advanced_widget;
use iced::advanced::widget::operation::{Operation, Scrollable as ScrollableOperation};
use iced::widget::scrollable;
use iced::{mouse, Rectangle, Task, Vector};

use super::{FileBrowser, COLUMN_BROWSER_WHEEL_LINE_PIXELS};
use crate::model::Message;
use crate::view::column_browser_scroll_id;

struct ColumnBrowserScrollBy {
    target: advanced_widget::Id,
    delta_x: f32,
}

impl ColumnBrowserScrollBy {
    fn new(target: iced::widget::Id, delta_x: f32) -> Self {
        Self {
            target: target.into(),
            delta_x,
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

impl FileBrowser {
    pub(super) fn focus_latest_column(&self) -> Task<Message> {
        iced::widget::operation::snap_to(
            column_browser_scroll_id(self.active_pane_id()),
            scrollable::RelativeOffset { x: 1.0, y: 0.0 },
        )
    }

    pub(super) fn handle_column_browser_wheel_scrolled(
        &self,
        delta: mouse::ScrollDelta,
    ) -> Task<Message> {
        if !self.is_cursor_over_column_browser {
            return Task::none();
        }

        let Some(delta_x) = self.column_browser_horizontal_wheel_delta(delta) else {
            return Task::none();
        };

        advanced_widget::operate(ColumnBrowserScrollBy::new(
            column_browser_scroll_id(self.active_pane_id()),
            delta_x,
        ))
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
}
