#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use iced::advanced::widget as advanced_widget;
use iced::advanced::{layout, overlay, renderer, Clipboard, Layout, Shell, Widget};
use iced::widget::scrollable;
use iced::{mouse, Element, Event, Length, Rectangle, Size, Task, Vector};

use super::FileBrowser;
use crate::model::{Message, ScrollbarRegion};

const MOS_SCROLL_STEP: f32 = 33.6;
const MOS_SCROLL_SPEED: f32 = 2.70;
const MOS_SCROLL_DURATION_TRANSITION: f32 = 0.095;
const MOS_SCROLL_DEAD_ZONE: f32 = 0.1;
const MOS_SCROLL_FILTER_WEIGHT: f32 = 0.23;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct SmoothScrollDelta {
    x: f32,
    y: f32,
}

impl SmoothScrollDelta {
    fn is_resting(self) -> bool {
        self.x.abs() <= f32::EPSILON && self.y.abs() <= f32::EPSILON
    }

    fn magnitude(self) -> f32 {
        self.x.abs().max(self.y.abs())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WheelScrollMode {
    MosAnimated,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct WheelScrollDelta {
    delta: SmoothScrollDelta,
    mode: WheelScrollMode,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MosScrollState {
    active_region: Option<ScrollbarRegion>,
    current: SmoothScrollDelta,
    buffer: SmoothScrollDelta,
    last_input: SmoothScrollDelta,
    filter: MosScrollFilter,
}

impl MosScrollState {
    fn push_wheel_delta(&mut self, region: ScrollbarRegion, delta: SmoothScrollDelta) {
        if self.active_region.as_ref() != Some(&region) {
            self.active_region = Some(region);
            self.current = SmoothScrollDelta::default();
            self.buffer = SmoothScrollDelta::default();
            self.last_input = SmoothScrollDelta::default();
            self.filter = MosScrollFilter::default();
        }

        update_mos_axis(
            &mut self.current.y,
            &mut self.buffer.y,
            self.last_input.y,
            delta.y,
        );
        update_mos_axis(
            &mut self.current.x,
            &mut self.buffer.x,
            self.last_input.x,
            delta.x,
        );
        self.last_input = delta;
    }

    fn next_frame_delta(&mut self) -> Option<(ScrollbarRegion, SmoothScrollDelta)> {
        let frame = SmoothScrollDelta {
            x: (self.buffer.x - self.current.x) * MOS_SCROLL_DURATION_TRANSITION,
            y: (self.buffer.y - self.current.y) * MOS_SCROLL_DURATION_TRANSITION,
        };

        self.current.x += frame.x;
        self.current.y += frame.y;

        let filtered = self.filter.fill(frame);
        if filtered.magnitude() <= MOS_SCROLL_DEAD_ZONE {
            return None;
        }

        self.active_region.clone().map(|region| (region, filtered))
    }

    fn is_active(&self) -> bool {
        if self.active_region.is_none() {
            return false;
        }

        let residual = SmoothScrollDelta {
            x: self.buffer.x - self.current.x,
            y: self.buffer.y - self.current.y,
        };

        residual.magnitude() > MOS_SCROLL_DEAD_ZONE
            || self.filter.pending().magnitude() > MOS_SCROLL_DEAD_ZONE
    }

    fn stop(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Default)]
struct MosScrollFilter {
    next_output: SmoothScrollDelta,
}

impl MosScrollFilter {
    fn fill(&mut self, frame: SmoothScrollDelta) -> SmoothScrollDelta {
        let output = self.next_output;
        self.next_output.x = polish_filter_axis(self.next_output.x, frame.x);
        self.next_output.y = polish_filter_axis(self.next_output.y, frame.y);
        output
    }

    fn pending(&self) -> SmoothScrollDelta {
        self.next_output
    }
}

impl FileBrowser {
    pub(crate) fn smooth_scroll_shift_pressed(&self) -> bool {
        self.keyboard_modifiers.shift()
    }

    pub(super) fn handle_smooth_scroll_wheel(
        &mut self,
        region: ScrollbarRegion,
        delta: mouse::ScrollDelta,
    ) -> Task<Message> {
        let Some(scroll_delta) = self.smooth_scroll_delta_for_region(&region, delta) else {
            return Task::none();
        };

        match scroll_delta.mode {
            WheelScrollMode::MosAnimated => {
                self.smooth_scroll
                    .push_wheel_delta(region.clone(), scroll_delta.delta);

                self.show_scrollbars_temporarily(region)
            }
            WheelScrollMode::Direct => {
                self.smooth_scroll.stop();
                let scroll_task = iced::widget::operation::scroll_by(
                    smooth_scroll_id(&region),
                    scroll_frame_offset(scroll_delta.delta),
                );

                Task::batch([self.show_scrollbars_temporarily(region), scroll_task])
            }
        }
    }

    pub(super) fn smooth_scroll_animation_is_active(&self) -> bool {
        self.smooth_scroll.is_active()
    }

    pub(super) fn advance_smooth_scroll_animation(&mut self) -> Task<Message> {
        let scroll_task = self
            .smooth_scroll
            .next_frame_delta()
            .map(|(region, delta)| {
                iced::widget::operation::scroll_by(
                    smooth_scroll_id(&region),
                    scroll_frame_offset(delta),
                )
            });

        if !self.smooth_scroll.is_active() {
            self.smooth_scroll.stop();
        }

        scroll_task.unwrap_or_else(Task::none)
    }

    fn smooth_scroll_delta_for_region(
        &self,
        region: &ScrollbarRegion,
        delta: mouse::ScrollDelta,
    ) -> Option<WheelScrollDelta> {
        wheel_delta_for_region(region, self.keyboard_modifiers.shift(), delta)
    }
}

pub(crate) fn smooth_scroll_content<'a>(
    content: impl Into<Element<'a, Message>>,
    region: ScrollbarRegion,
) -> Element<'a, Message> {
    smooth_scroll_content_with_shift(content, region, false)
}

pub(crate) fn smooth_scroll_content_with_shift<'a>(
    content: impl Into<Element<'a, Message>>,
    region: ScrollbarRegion,
    shift_pressed: bool,
) -> Element<'a, Message> {
    Element::new(SmoothScrollArea {
        content: content.into(),
        region,
        shift_pressed,
    })
}

struct SmoothScrollArea<'a> {
    content: Element<'a, Message>,
    region: ScrollbarRegion,
    shift_pressed: bool,
}

impl Widget<Message, iced::Theme, iced::Renderer> for SmoothScrollArea<'_> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut advanced_widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn children(&self) -> Vec<advanced_widget::Tree> {
        vec![advanced_widget::Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut advanced_widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn operate(
        &mut self,
        tree: &mut advanced_widget::Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut advanced_widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let cursor_over_area = cursor.is_over(layout.bounds());

        // 嵌套滚动区必须让内层先认领 wheel；否则 Settings 会吞掉 ShortcutSettings。
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        if shell.is_event_captured() {
            return;
        }

        if cursor_over_area {
            if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event {
                if wheel_delta_for_region(&self.region, self.shift_pressed, *delta).is_some() {
                    shell.publish(Message::SmoothScrollWheel(self.region.clone(), *delta));
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &advanced_widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &advanced_widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut advanced_widget::Tree,
        layout: Layout<'a>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

pub(crate) fn smooth_scroll_id(region: &ScrollbarRegion) -> iced::widget::Id {
    match region {
        ScrollbarRegion::Sidebar => iced::widget::Id::new("sidebar"),
        ScrollbarRegion::PaneList(pane_id) => {
            iced::widget::Id::from(format!("pane-list-{}", pane_id.key()))
        }
        ScrollbarRegion::ColumnBrowser(pane_id) => {
            iced::widget::Id::from(format!("column-browser-{}", pane_id.key()))
        }
        ScrollbarRegion::Column { pane_id, directory } => iced::widget::Id::from(format!(
            "column-scroll-{}-{}",
            pane_id.key(),
            path_hash(directory)
        )),
        ScrollbarRegion::Settings => iced::widget::Id::new("settings"),
        ScrollbarRegion::ShortcutSettings => iced::widget::Id::new("shortcut-settings"),
        ScrollbarRegion::Properties => iced::widget::Id::new("properties"),
        ScrollbarRegion::OpenWithApplications => iced::widget::Id::new("open-with-applications"),
        ScrollbarRegion::OperationQueue => iced::widget::Id::new("operation-queue"),
        ScrollbarRegion::BatchRenamePreview => iced::widget::Id::new("batch-rename-preview"),
        ScrollbarRegion::SearchResults => iced::widget::Id::new("search-results"),
        ScrollbarRegion::PreviewDirectory => iced::widget::Id::new("preview-directory"),
        ScrollbarRegion::PreviewArchive => iced::widget::Id::new("preview-archive"),
        ScrollbarRegion::MarkdownPreview => iced::widget::Id::new("markdown-preview"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmoothScrollAxis {
    Vertical,
    Horizontal,
}

fn smooth_scroll_axis(region: &ScrollbarRegion) -> SmoothScrollAxis {
    match region {
        ScrollbarRegion::ColumnBrowser(_) => SmoothScrollAxis::Horizontal,
        _ => SmoothScrollAxis::Vertical,
    }
}

fn wheel_delta_for_region(
    region: &ScrollbarRegion,
    shift_pressed: bool,
    delta: mouse::ScrollDelta,
) -> Option<WheelScrollDelta> {
    let delta = match smooth_scroll_axis(region) {
        SmoothScrollAxis::Vertical => vertical_wheel_delta(shift_pressed, delta),
        SmoothScrollAxis::Horizontal => horizontal_wheel_delta(shift_pressed, delta),
    };

    (!delta.delta.is_resting()).then_some(delta)
}

fn vertical_wheel_delta(shift_pressed: bool, delta: mouse::ScrollDelta) -> WheelScrollDelta {
    match delta {
        mouse::ScrollDelta::Lines { x, y } => WheelScrollDelta {
            delta: SmoothScrollDelta {
                x: 0.0,
                y: -if shift_pressed { x } else { y } * MOS_SCROLL_STEP,
            },
            mode: WheelScrollMode::MosAnimated,
        },
        mouse::ScrollDelta::Pixels { x, y } => WheelScrollDelta {
            delta: SmoothScrollDelta {
                x: 0.0,
                y: -if shift_pressed { x } else { y },
            },
            mode: WheelScrollMode::Direct,
        },
    }
}

fn horizontal_wheel_delta(shift_pressed: bool, delta: mouse::ScrollDelta) -> WheelScrollDelta {
    match delta {
        mouse::ScrollDelta::Lines { x, y } => WheelScrollDelta {
            delta: SmoothScrollDelta {
                x: -if shift_pressed { y } else { x } * MOS_SCROLL_STEP,
                y: 0.0,
            },
            mode: WheelScrollMode::MosAnimated,
        },
        mouse::ScrollDelta::Pixels { x, y } => WheelScrollDelta {
            delta: SmoothScrollDelta {
                x: -if shift_pressed { y } else { x },
                y: 0.0,
            },
            mode: WheelScrollMode::Direct,
        },
    }
}

fn update_mos_axis(current: &mut f32, buffer: &mut f32, last_input: f32, incoming: f32) {
    let scaled = incoming * MOS_SCROLL_SPEED;
    if incoming * last_input > 0.0 {
        *buffer += scaled;
    } else {
        *buffer = scaled;
        *current = 0.0;
    }
}

fn polish_filter_axis(current: f32, next: f32) -> f32 {
    current + MOS_SCROLL_FILTER_WEIGHT * (next - current)
}

fn scroll_frame_offset(delta: SmoothScrollDelta) -> scrollable::AbsoluteOffset {
    scrollable::AbsoluteOffset {
        x: delta.x,
        y: delta.y,
    }
}

pub(crate) fn path_hash(path: &Path) -> String {
    let mut hasher = blake3::Hasher::new();
    #[cfg(unix)]
    {
        hasher.update(path.as_os_str().as_bytes());
    }
    #[cfg(not(unix))]
    {
        hasher.update(path.to_string_lossy().as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::model::BrowserPaneId;

    #[test]
    fn vertical_line_delta_matches_native_wheel_direction() {
        let delta = vertical_wheel_delta(false, mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 });

        assert_eq!(
            delta,
            WheelScrollDelta {
                delta: SmoothScrollDelta {
                    x: 0.0,
                    y: MOS_SCROLL_STEP,
                },
                mode: WheelScrollMode::MosAnimated,
            },
        );
    }

    #[test]
    fn vertical_pixel_delta_preserves_native_wheel_delta() {
        let delta = vertical_wheel_delta(false, mouse::ScrollDelta::Pixels { x: 0.0, y: -4.0 });

        assert_eq!(
            delta,
            WheelScrollDelta {
                delta: SmoothScrollDelta { x: 0.0, y: 4.0 },
                mode: WheelScrollMode::Direct,
            }
        );
    }

    #[test]
    fn horizontal_region_ignores_unshifted_vertical_wheel() {
        let delta = wheel_delta_for_region(
            &ScrollbarRegion::ColumnBrowser(BrowserPaneId::PRIMARY),
            false,
            mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
        );

        assert_eq!(delta, None);
    }

    #[test]
    fn column_region_ignores_shifted_vertical_wheel() {
        let delta = wheel_delta_for_region(
            &ScrollbarRegion::Column {
                pane_id: BrowserPaneId::PRIMARY,
                directory: PathBuf::from("/tmp"),
            },
            true,
            mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
        );

        assert_eq!(delta, None);
    }

    #[test]
    fn shifted_column_browser_uses_vertical_wheel_for_horizontal_delta() {
        let delta = wheel_delta_for_region(
            &ScrollbarRegion::ColumnBrowser(BrowserPaneId::PRIMARY),
            true,
            mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 },
        );

        assert_eq!(
            delta,
            Some(WheelScrollDelta {
                delta: SmoothScrollDelta {
                    x: MOS_SCROLL_STEP * 2.0,
                    y: 0.0,
                },
                mode: WheelScrollMode::MosAnimated,
            })
        );
    }

    #[test]
    fn shifted_column_browser_uses_vertical_wheel_for_horizontal_scroll() {
        let delta = horizontal_wheel_delta(true, mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 });

        assert_eq!(
            delta,
            WheelScrollDelta {
                delta: SmoothScrollDelta {
                    x: MOS_SCROLL_STEP * 2.0,
                    y: 0.0,
                },
                mode: WheelScrollMode::MosAnimated,
            }
        );
    }

    #[test]
    fn mos_global_state_accumulates_same_region_buffer() {
        let mut state = MosScrollState::default();
        let region = ScrollbarRegion::Sidebar;
        state.push_wheel_delta(
            region.clone(),
            SmoothScrollDelta {
                x: 0.0,
                y: MOS_SCROLL_STEP,
            },
        );
        state.push_wheel_delta(
            region,
            SmoothScrollDelta {
                x: 0.0,
                y: MOS_SCROLL_STEP,
            },
        );

        assert!((state.buffer.y - MOS_SCROLL_STEP * MOS_SCROLL_SPEED * 2.0).abs() <= 0.0001);
        assert_eq!(state.current.y, 0.0);
    }

    #[test]
    fn mos_global_state_resets_current_on_opposite_direction() {
        let mut state = MosScrollState::default();
        let region = ScrollbarRegion::Sidebar;
        state.current.y = 12.0;
        state.push_wheel_delta(
            region.clone(),
            SmoothScrollDelta {
                x: 0.0,
                y: MOS_SCROLL_STEP,
            },
        );
        state.push_wheel_delta(
            region,
            SmoothScrollDelta {
                x: 0.0,
                y: -MOS_SCROLL_STEP,
            },
        );

        assert!((state.buffer.y + MOS_SCROLL_STEP * MOS_SCROLL_SPEED).abs() <= 0.0001);
        assert_eq!(state.current.y, 0.0);
    }

    #[test]
    fn mos_filter_delays_first_scroll_frame() {
        let mut state = MosScrollState::default();
        state.push_wheel_delta(
            ScrollbarRegion::Sidebar,
            SmoothScrollDelta {
                x: 0.0,
                y: MOS_SCROLL_STEP,
            },
        );

        assert_eq!(state.next_frame_delta(), None);
        let (_, second_frame) = state.next_frame_delta().expect("second frame");

        assert!(second_frame.y > MOS_SCROLL_DEAD_ZONE);
    }

    #[test]
    fn mos_filter_preserves_fractional_line_wheel_delta() {
        let mut state = MosScrollState::default();
        state.push_wheel_delta(
            ScrollbarRegion::Sidebar,
            SmoothScrollDelta {
                x: 0.0,
                y: MOS_SCROLL_STEP * 0.1,
            },
        );

        assert_eq!(state.next_frame_delta(), None);
        let (_, second_frame) = state.next_frame_delta().expect("second frame");

        assert!(second_frame.y > MOS_SCROLL_DEAD_ZONE);
    }

    #[test]
    fn mos_global_state_switches_active_region() {
        let mut state = MosScrollState::default();
        state.push_wheel_delta(
            ScrollbarRegion::Sidebar,
            SmoothScrollDelta {
                x: 0.0,
                y: MOS_SCROLL_STEP,
            },
        );
        state.current.y = 12.0;

        let next_region = ScrollbarRegion::PaneList(BrowserPaneId::PRIMARY);
        state.push_wheel_delta(
            next_region.clone(),
            SmoothScrollDelta {
                x: 0.0,
                y: MOS_SCROLL_STEP,
            },
        );

        assert_eq!(state.active_region, Some(next_region));
        assert_eq!(state.current.y, 0.0);
        assert!((state.buffer.y - MOS_SCROLL_STEP * MOS_SCROLL_SPEED).abs() <= 0.0001);
    }

    #[test]
    fn frame_offset_preserves_relative_scroll_delta() {
        let offset = scroll_frame_offset(SmoothScrollDelta { x: -3.0, y: 6.0 });

        assert_eq!(offset.x, -3.0);
        assert_eq!(offset.y, 6.0);
    }
}
