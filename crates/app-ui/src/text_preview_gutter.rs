use iced::advanced::text::editor::Editor;
use iced::advanced::text::highlighter::PlainText;
use iced::advanced::text::{Paragraph, Renderer as TextRenderer};
use iced::advanced::{
    graphics, layout, overlay, renderer, text, widget, Clipboard, Layout, Shell, Widget,
};
use iced::widget::text_editor;
use iced::{
    alignment, mouse, Color, Element, Event, Font, Length, Padding, Pixels, Point, Rectangle, Size,
    Theme, Vector,
};

use crate::text_preview::{TextPreviewDocument, TEXT_PREVIEW_LINE_HEIGHT, TEXT_PREVIEW_TEXT_SIZE};

const TEXT_PREVIEW_EDITOR_PADDING: f32 = 8.0;

type PreviewEditor = graphics::text::Editor;
type PreviewParagraph = graphics::text::Paragraph;

pub(crate) fn text_preview_with_gutter<'a, Message>(
    document: &'a TextPreviewDocument,
    content: impl Into<Element<'a, Message>>,
    scroll_height: f32,
    gutter_width: f32,
    gutter_spacing: f32,
    on_gutter_scroll: impl Fn(mouse::ScrollDelta) -> Message + 'a,
) -> Element<'a, Message>
where
    Message: 'a,
{
    Element::new(TextPreviewGutter {
        document,
        content: content.into(),
        scroll_height,
        gutter_width,
        gutter_spacing,
        on_gutter_scroll: Box::new(on_gutter_scroll),
    })
}

struct TextPreviewGutter<'a, Message> {
    document: &'a TextPreviewDocument,
    content: Element<'a, Message>,
    scroll_height: f32,
    gutter_width: f32,
    gutter_spacing: f32,
    on_gutter_scroll: Box<dyn Fn(mouse::ScrollDelta) -> Message + 'a>,
}

#[derive(Default)]
struct TextPreviewGutterState {
    mirror_editor: PreviewEditor,
    mirror_content_revision: Option<u64>,
    mirror_content_width: f32,
    mirror_scroll_height: f32,
    // Iced/WGPU 会把 paragraph 降级成弱引用后延迟 prepare；行号 paragraph 必须由 widget state 持有到渲染阶段。
    retained_line_numbers: Vec<TextPreviewLineNumber>,
    retained_line_number_visual_scroll_line_offset: Option<usize>,
    retained_line_number_mirror_scroll_y: f32,
    retained_line_number_digit_count: usize,
    retained_line_number_gutter_width: f32,
}

struct TextPreviewLineNumber {
    y: f32,
    paragraph: PreviewParagraph,
}

impl<'a, Message> Widget<Message, Theme, iced::Renderer> for TextPreviewGutter<'a, Message>
where
    Message: 'a,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<TextPreviewGutterState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(TextPreviewGutterState::default())
    }

    fn children(&self) -> Vec<widget::Tree> {
        vec![widget::Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(self.scroll_height))
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits
            .width(Length::Fill)
            .height(Length::Fixed(self.scroll_height))
            .resolve(Length::Fill, Length::Fixed(self.scroll_height), Size::ZERO);
        let content_offset = self.gutter_width + self.gutter_spacing;
        let content_width = (size.width - content_offset).max(0.0);
        let content_limits =
            layout::Limits::new(Size::ZERO, Size::new(content_width, self.scroll_height))
                .width(Length::Fixed(content_width))
                .height(Length::Fixed(self.scroll_height));
        let content_node = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &content_limits)
            .move_to(Point::new(content_offset, 0.0));

        let gutter_state = tree.state.downcast_mut::<TextPreviewGutterState>();
        let mirror_will_update = mirror_editor_needs_update(
            gutter_state,
            self.document.content_revision(),
            content_width,
            self.scroll_height,
        );
        update_mirror_editor(
            gutter_state,
            self.document,
            content_width,
            self.scroll_height,
        );
        update_retained_line_numbers(
            gutter_state,
            self.document,
            self.gutter_width,
            self.scroll_height,
            mirror_will_update,
        );

        layout::Node::with_children(size, vec![content_node])
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout.child(0),
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: iced::mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.child(0),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        if shell.is_event_captured() {
            if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event {
                apply_scroll_delta_to_mirror_editor(
                    tree.state.downcast_mut::<TextPreviewGutterState>(),
                    *delta,
                );
            }
            return;
        }

        if cursor.is_over(gutter_bounds(layout.bounds(), self.gutter_width)) {
            if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event {
                apply_scroll_delta_to_mirror_editor(
                    tree.state.downcast_mut::<TextPreviewGutterState>(),
                    *delta,
                );
                shell.publish((self.on_gutter_scroll)(*delta));
                shell.capture_event();
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: iced::mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> iced::mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout.child(0),
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: iced::mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let gutter_state = tree.state.downcast_ref::<TextPreviewGutterState>();
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout.child(0),
            cursor,
            viewport,
        );

        let bounds = layout.bounds();
        let gutter_bounds = gutter_bounds(bounds, self.gutter_width);
        let clip_bounds = gutter_bounds.intersection(viewport);

        let Some(clip_bounds) = clip_bounds else {
            return;
        };
        let line_number_color = muted_line_number_color(theme);
        let text_origin_y = bounds.y + TEXT_PREVIEW_EDITOR_PADDING;

        for line_number in &gutter_state.retained_line_numbers {
            let line_bounds =
                line_number_bounds(bounds, self.gutter_width, text_origin_y + line_number.y);
            let position = line_bounds.anchor(
                line_number.paragraph.min_bounds(),
                alignment::Horizontal::Right,
                alignment::Vertical::Top,
            );
            renderer.fill_paragraph(
                &line_number.paragraph,
                position,
                line_number_color,
                clip_bounds,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout.child(0),
            renderer,
            viewport,
            translation,
        )
    }
}

fn update_mirror_editor(
    state: &mut TextPreviewGutterState,
    document: &TextPreviewDocument,
    width: f32,
    height: f32,
) {
    let content_revision = document.content_revision();
    if state.mirror_content_revision == Some(content_revision)
        && (state.mirror_content_width - width).abs() < f32::EPSILON
        && (state.mirror_scroll_height - height).abs() < f32::EPSILON
    {
        return;
    }

    state.mirror_editor = PreviewEditor::with_text(&document.content().text());
    state.mirror_content_revision = Some(content_revision);
    state.mirror_content_width = width;
    state.mirror_scroll_height = height;
    let padding = Padding::new(TEXT_PREVIEW_EDITOR_PADDING);
    state.mirror_editor.update(
        Size::new(
            (width - padding.x()).max(0.0),
            (height - padding.y()).max(0.0),
        ),
        Font::MONOSPACE,
        Pixels(TEXT_PREVIEW_TEXT_SIZE),
        text::LineHeight::Relative(TEXT_PREVIEW_LINE_HEIGHT),
        text::Wrapping::WordOrGlyph,
        &mut PlainText,
    );
    apply_scroll_lines_to_mirror_editor(
        state,
        document
            .visual_scroll_line_offset()
            .try_into()
            .unwrap_or(i32::MAX),
    );
}

fn mirror_editor_needs_update(
    state: &TextPreviewGutterState,
    content_revision: u64,
    width: f32,
    height: f32,
) -> bool {
    state.mirror_content_revision != Some(content_revision)
        || (state.mirror_content_width - width).abs() >= f32::EPSILON
        || (state.mirror_scroll_height - height).abs() >= f32::EPSILON
}

fn update_retained_line_numbers(
    state: &mut TextPreviewGutterState,
    document: &TextPreviewDocument,
    gutter_width: f32,
    scroll_height: f32,
    mirror_was_updated: bool,
) -> bool {
    let visual_scroll_line_offset = document.visual_scroll_line_offset();
    let mirror_scroll_y = mirror_editor_scroll_y(&state.mirror_editor);
    let digit_count = document.line_number_digit_count();
    let line_numbers_need_update = mirror_was_updated
        || state.retained_line_number_visual_scroll_line_offset != Some(visual_scroll_line_offset)
        || (state.retained_line_number_mirror_scroll_y - mirror_scroll_y).abs() >= f32::EPSILON
        || state.retained_line_number_digit_count != digit_count
        || (state.retained_line_number_gutter_width - gutter_width).abs() >= f32::EPSILON;

    if !line_numbers_need_update {
        return false;
    }

    let offsets = line_number_offsets(&state.mirror_editor, scroll_height);

    state.retained_line_numbers = offsets
        .into_iter()
        .map(|(line_index, y)| TextPreviewLineNumber {
            y,
            paragraph: line_number_paragraph(line_index + 1, digit_count, gutter_width),
        })
        .collect();
    state.retained_line_number_visual_scroll_line_offset = Some(visual_scroll_line_offset);
    state.retained_line_number_mirror_scroll_y = mirror_scroll_y;
    state.retained_line_number_digit_count = digit_count;
    state.retained_line_number_gutter_width = gutter_width;
    true
}

fn apply_scroll_delta_to_mirror_editor(
    state: &mut TextPreviewGutterState,
    delta: mouse::ScrollDelta,
) -> i32 {
    let lines = text_preview_scroll_lines(delta);
    apply_scroll_lines_to_mirror_editor(state, lines);
    lines
}

fn apply_scroll_lines_to_mirror_editor(state: &mut TextPreviewGutterState, lines: i32) {
    if lines == 0 {
        return;
    }

    state
        .mirror_editor
        .perform(text_editor::Action::Scroll { lines });
}

fn text_preview_scroll_lines(delta: mouse::ScrollDelta) -> i32 {
    let lines = match delta {
        mouse::ScrollDelta::Lines { y, .. } => {
            if y.abs() > 0.0 {
                y.signum() * -(y.abs() * 4.0).max(1.0)
            } else {
                0.0
            }
        }
        mouse::ScrollDelta::Pixels { y, .. } => -y / 4.0,
    };

    lines as i32
}

fn gutter_bounds(bounds: Rectangle, gutter_width: f32) -> Rectangle {
    Rectangle {
        width: gutter_width,
        ..bounds
    }
}

fn line_number_bounds(bounds: Rectangle, gutter_width: f32, y: f32) -> Rectangle {
    Rectangle {
        x: bounds.x,
        y,
        width: gutter_width,
        height: TEXT_PREVIEW_TEXT_SIZE * TEXT_PREVIEW_LINE_HEIGHT,
    }
}

fn line_number_paragraph(
    line_number: usize,
    digit_count: usize,
    gutter_width: f32,
) -> PreviewParagraph {
    let size = Pixels(TEXT_PREVIEW_TEXT_SIZE);
    let line_height = text::LineHeight::Relative(TEXT_PREVIEW_LINE_HEIGHT);
    let label = format!("{line_number:>digit_count$}");
    PreviewParagraph::with_text(text::Text {
        content: label.as_str(),
        bounds: Size::new(gutter_width, line_height.to_absolute(size).0),
        size,
        line_height,
        font: Font::MONOSPACE,
        align_x: text::Alignment::Right,
        align_y: alignment::Vertical::Top,
        shaping: text::Shaping::Basic,
        wrapping: text::Wrapping::None,
    })
}

fn line_number_offsets(editor: &PreviewEditor, scroll_height: f32) -> Vec<(usize, f32)> {
    let buffer = editor.buffer();
    let metrics = buffer.metrics();
    let mut offsets = Vec::new();
    let scroll_offset = buffer.scroll().vertical;
    let mut y = 0.0;

    for (line_index, line) in buffer.lines.iter().enumerate() {
        let line_height = line
            .layout_opt()
            .map(|layout| {
                layout
                    .iter()
                    .map(|layout_line| layout_line.line_height_opt.unwrap_or(metrics.line_height))
                    .sum()
            })
            .unwrap_or(metrics.line_height);

        let visible_y = y - scroll_offset;
        if visible_y + line_height >= 0.0 {
            offsets.push((line_index, visible_y));
        }
        if visible_y > scroll_height {
            break;
        }

        y += line_height;
    }

    offsets
}

fn mirror_editor_scroll_y(editor: &PreviewEditor) -> f32 {
    editor.buffer().scroll().vertical
}

fn muted_line_number_color(theme: &Theme) -> Color {
    let background = theme.palette().background;
    let is_dark = background.r * 0.299 + background.g * 0.587 + background.b * 0.114 < 0.5;
    if is_dark {
        Color::from_rgb8(137, 146, 159)
    } else {
        Color::from_rgb8(119, 127, 139)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview_editor_for_text(content: &str, width: f32, height: f32) -> PreviewEditor {
        let mut editor = PreviewEditor::with_text(content);
        let padding = Padding::new(TEXT_PREVIEW_EDITOR_PADDING);
        editor.update(
            Size::new(
                (width - padding.x()).max(0.0),
                (height - padding.y()).max(0.0),
            ),
            Font::MONOSPACE,
            Pixels(TEXT_PREVIEW_TEXT_SIZE),
            text::LineHeight::Relative(TEXT_PREVIEW_LINE_HEIGHT),
            text::Wrapping::WordOrGlyph,
            &mut PlainText,
        );
        editor
    }

    #[test]
    fn line_number_offsets_only_returns_visible_rows() {
        let content = (0..1_000)
            .map(|line_number| format!("line {line_number}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut editor = preview_editor_for_text(&content, 400.0, 120.0);
        editor.perform(text_editor::Action::Scroll { lines: 500 });

        let offsets = line_number_offsets(&editor, 120.0);

        assert!(offsets.len() < 12);
        assert!(offsets.iter().all(|(line_index, _)| *line_index < 510));
        assert!(offsets.iter().any(|(line_index, _)| *line_index >= 500));
    }

    #[test]
    fn wrapped_visual_lines_keep_one_number_per_real_line() {
        let content = format!("{}\nshort", "very_long_word".repeat(40));
        let editor = preview_editor_for_text(&content, 80.0, 600.0);

        let offsets = line_number_offsets(&editor, 600.0);

        assert_eq!(
            offsets
                .iter()
                .filter(|(line_index, _)| *line_index == 0)
                .count(),
            1
        );
        assert_eq!(
            offsets
                .iter()
                .filter(|(line_index, _)| *line_index == 1)
                .count(),
            1
        );
    }

    #[test]
    fn line_number_anchor_stays_inside_gutter_bounds() {
        let bounds = Rectangle {
            x: 20.0,
            y: 40.0,
            width: 240.0,
            height: 120.0,
        };
        let gutter_width = 36.0;
        let paragraph = line_number_paragraph(42, 2, gutter_width);
        let line_bounds = line_number_bounds(bounds, gutter_width, 48.0);
        let position = line_bounds.anchor(
            paragraph.min_bounds(),
            alignment::Horizontal::Right,
            alignment::Vertical::Top,
        );

        assert!(position.x >= line_bounds.x);
        assert!(position.x + paragraph.min_bounds().width <= line_bounds.x + line_bounds.width);
        assert_eq!(position.y, line_bounds.y);
    }
}
