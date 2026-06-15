use std::path::PathBuf;

use iced::advanced::clipboard::Kind as ClipboardKind;
use iced::advanced::mouse::{click, Click};
use iced::advanced::text::editor::{Editor as _, Selection};
use iced::advanced::text::highlighter::PlainText;
use iced::advanced::text::{Paragraph, Renderer as TextRenderer};
use iced::advanced::Renderer as _;
use iced::advanced::{graphics, layout, renderer, text, widget, Clipboard, Layout, Shell, Widget};
use iced::keyboard;
use iced::widget::text_editor;
use iced::{
    alignment, mouse, Color, Element, Event, Font, Length, Padding, Pixels, Point, Rectangle, Size,
    Theme,
};

use crate::text_preview::{TextPreviewDocument, TEXT_PREVIEW_LINE_HEIGHT, TEXT_PREVIEW_TEXT_SIZE};

const TEXT_PREVIEW_VIEWER_PADDING: f32 = 8.0;
const TEXT_PREVIEW_GUTTER_HORIZONTAL_PADDING: f32 = 6.0;
const TEXT_PREVIEW_GUTTER_DIGIT_WIDTH: f32 = 10.0;
const TEXT_PREVIEW_GUTTER_MIN_WIDTH: f32 = 30.0;
const TEXT_PREVIEW_GUTTER_SPACING: f32 = 8.0;
const TEXT_PREVIEW_DIVIDER_WIDTH: f32 = 1.0;

type PreviewEditor = graphics::text::Editor;
type PreviewParagraph = graphics::text::Paragraph;

pub(crate) fn text_preview_viewer<'a, Message>(
    document: &'a TextPreviewDocument,
    scroll_height: f32,
) -> Element<'a, Message>
where
    Message: 'a + 'static,
{
    Element::new(TextPreviewViewer {
        document,
        scroll_height,
    })
}

struct TextPreviewViewer<'a> {
    document: &'a TextPreviewDocument,
    scroll_height: f32,
}

#[derive(Default)]
struct TextPreviewViewerState {
    editor: PreviewEditor,
    source_path: Option<PathBuf>,
    generation: Option<u64>,
    content_revision: Option<u64>,
    content_width: f32,
    scroll_height: f32,
    partial_scroll: f32,
    last_click: Option<Click>,
    drag_click: Option<click::Kind>,
    is_focused: bool,
    retained_text_lines: Vec<TextPreviewVisibleTextLine>,
    retained_text_line_scroll_y: f32,
    retained_text_line_width: f32,
    retained_text_line_height: f32,
    retained_line_numbers: Vec<TextPreviewLineNumber>,
    retained_line_number_scroll_y: f32,
    retained_line_number_digit_count: usize,
    retained_line_number_gutter_width: f32,
    retained_line_number_height: f32,
}

struct TextPreviewVisibleTextLine {
    y: f32,
    paragraph: PreviewParagraph,
}

struct TextPreviewLineNumber {
    y: f32,
    paragraph: PreviewParagraph,
}

struct VisibleLogicalLine {
    line_index: usize,
    y: f32,
    height: f32,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for TextPreviewViewer<'_>
where
    Message: 'static,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<TextPreviewViewerState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(TextPreviewViewerState::default())
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
        let gutter_width = text_preview_line_number_gutter_width(self.document);
        let content_offset =
            gutter_width + TEXT_PREVIEW_GUTTER_SPACING + TEXT_PREVIEW_DIVIDER_WIDTH;
        let content_width = (size.width - content_offset).max(0.0);

        update_editor(
            tree.state.downcast_mut::<TextPreviewViewerState>(),
            self.document,
            content_width,
            self.scroll_height,
        );
        let _ = renderer;
        layout::Node::new(size)
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<TextPreviewViewerState>();
        let bounds = layout.bounds();
        let text_bounds = text_bounds(bounds, self.document);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(position) = cursor.position_in(text_bounds) else {
                    if state.is_focused && !cursor.is_over(bounds) {
                        state.is_focused = false;
                        state.drag_click = None;
                    }
                    return;
                };

                let click = Click::new(position, mouse::Button::Left, state.last_click);
                let action = match click.kind() {
                    click::Kind::Single => text_editor::Action::Click(position),
                    click::Kind::Double => text_editor::Action::SelectWord,
                    click::Kind::Triple => text_editor::Action::SelectLine,
                };

                state.editor.perform(action);
                state.is_focused = true;
                state.last_click = Some(click);
                state.drag_click = Some(click.kind());
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.drag_click != Some(click::Kind::Single) {
                    return;
                }

                let Some(position) = cursor.position_in(text_bounds) else {
                    return;
                };

                state.editor.perform(text_editor::Action::Drag(position));
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag_click.take().is_some() {
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let lines = text_preview_scroll_lines(*delta) + state.partial_scroll;
                let line_count = lines as i32;
                if line_count != 0 {
                    if apply_bounded_scroll_lines(state, line_count, text_bounds.height) {
                        state.partial_scroll = lines.fract();
                        update_retained_visible_content(state, self.document, bounds);
                        shell.request_redraw();
                    } else {
                        state.partial_scroll = 0.0;
                    }
                }
                shell.capture_event();
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modified_key,
                physical_key,
                modifiers,
                text,
                ..
            }) if state.is_focused => {
                if let Some(binding) =
                    text_editor::Binding::<Message>::from_key_press(text_editor::KeyPress {
                        key: key.clone(),
                        modified_key: modified_key.clone(),
                        physical_key: physical_key.clone(),
                        modifiers: *modifiers,
                        text: text.clone(),
                        status: text_editor::Status::Focused {
                            is_hovered: cursor.is_over(bounds),
                        },
                    })
                {
                    apply_key_binding(binding, &mut state.editor, clipboard);
                    clamp_editor_scroll_to_bounds(state, text_bounds.height);
                    update_retained_visible_content(state, self.document, bounds);
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(text_bounds(layout.bounds(), self.document)) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<TextPreviewViewerState>();
        let bounds = layout.bounds();
        let gutter_width = text_preview_line_number_gutter_width(self.document);
        let gutter_bounds = Rectangle {
            width: gutter_width,
            ..bounds
        };
        let text_bounds = text_bounds(bounds, self.document);
        let divider_bounds = Rectangle {
            x: bounds.x + gutter_width + TEXT_PREVIEW_GUTTER_SPACING / 2.0,
            y: bounds.y,
            width: TEXT_PREVIEW_DIVIDER_WIDTH,
            height: bounds.height,
        };

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..renderer::Quad::default()
            },
            viewer_background_color(theme),
        );
        renderer.fill_quad(
            renderer::Quad {
                bounds: divider_bounds,
                ..renderer::Quad::default()
            },
            divider_color(theme),
        );

        if state.editor.is_empty() {
            renderer.fill_text(
                text::Text {
                    content: "(empty file)".into(),
                    bounds: text_bounds.size(),
                    size: Pixels(TEXT_PREVIEW_TEXT_SIZE),
                    line_height: text::LineHeight::Relative(TEXT_PREVIEW_LINE_HEIGHT),
                    font: Font::MONOSPACE,
                    align_x: text::Alignment::Default,
                    align_y: alignment::Vertical::Top,
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::WordOrGlyph,
                },
                text_bounds.position(),
                placeholder_color(theme),
                text_bounds,
            );
        } else {
            draw_selection_ranges(renderer, &state.editor, text_bounds, theme);
            draw_visible_text_lines(renderer, state, text_bounds, theme, viewport);
        }

        let Some(clip_bounds) = gutter_bounds.intersection(viewport) else {
            return;
        };
        let line_number_color = placeholder_color(theme);
        let text_origin_y = bounds.y + TEXT_PREVIEW_VIEWER_PADDING;
        for line_number in &state.retained_line_numbers {
            let line_bounds = line_number_bounds(gutter_bounds, text_origin_y + line_number.y);
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
}

fn update_editor(
    state: &mut TextPreviewViewerState,
    document: &TextPreviewDocument,
    content_width: f32,
    scroll_height: f32,
) {
    let needs_source_update = state.source_path.as_deref() != Some(document.path())
        || state.generation != Some(document.generation())
        || state.content_revision != Some(document.content_revision());
    let needs_layout_update = (state.content_width - content_width).abs() >= f32::EPSILON
        || (state.scroll_height - scroll_height).abs() >= f32::EPSILON;

    if !needs_source_update && !needs_layout_update {
        return;
    }

    if needs_source_update {
        state.editor = PreviewEditor::with_text(&document.content_text());
        state.source_path = Some(document.path().to_path_buf());
        state.generation = Some(document.generation());
        state.content_revision = Some(document.content_revision());
        state.partial_scroll = 0.0;
        state.last_click = None;
        state.drag_click = None;
        state.is_focused = false;
        state.retained_text_lines.clear();
        state.retained_line_numbers.clear();
    }

    state.content_width = content_width;
    state.scroll_height = scroll_height;
    let padding = Padding::new(TEXT_PREVIEW_VIEWER_PADDING);
    let editor_size = Size::new(
        (content_width - padding.x()).max(0.0),
        (scroll_height - padding.y()).max(0.0),
    );
    state.editor.update(
        editor_size,
        Font::MONOSPACE,
        Pixels(TEXT_PREVIEW_TEXT_SIZE),
        text::LineHeight::Relative(TEXT_PREVIEW_LINE_HEIGHT),
        text::Wrapping::WordOrGlyph,
        &mut PlainText,
    );
    clamp_editor_scroll_to_bounds(state, editor_size.height);
    update_retained_text_lines_for_size(state, editor_size.width, editor_size.height);
    update_retained_line_numbers_for_size(
        state,
        document.line_number_digit_count(),
        text_preview_line_number_gutter_width(document),
        editor_size.height,
    );
}

fn update_retained_visible_content(
    state: &mut TextPreviewViewerState,
    document: &TextPreviewDocument,
    bounds: Rectangle,
) {
    let text_bounds = text_bounds(bounds, document);
    update_retained_text_lines_for_size(state, text_bounds.width, text_bounds.height);
    update_retained_line_numbers_for_size(
        state,
        document.line_number_digit_count(),
        text_preview_line_number_gutter_width(document),
        text_bounds.height,
    );
}

fn update_retained_text_lines_for_size(
    state: &mut TextPreviewViewerState,
    text_width: f32,
    text_height: f32,
) {
    let scroll_y = editor_scroll_y(&state.editor);
    let text_lines_need_update = (state.retained_text_line_scroll_y - scroll_y).abs()
        >= f32::EPSILON
        || (state.retained_text_line_width - text_width).abs() >= f32::EPSILON
        || (state.retained_text_line_height - text_height).abs() >= f32::EPSILON;
    if !text_lines_need_update && !state.retained_text_lines.is_empty() {
        return;
    }

    state.retained_text_lines = visible_logical_lines(&state.editor, text_height)
        .into_iter()
        .filter_map(|visible_line| {
            state
                .editor
                .line(visible_line.line_index)
                .map(|line| TextPreviewVisibleTextLine {
                    y: visible_line.y,
                    paragraph: text_line_paragraph(
                        line.text.as_ref(),
                        text_width,
                        visible_line.height,
                    ),
                })
        })
        .collect();
    state.retained_text_line_scroll_y = scroll_y;
    state.retained_text_line_width = text_width;
    state.retained_text_line_height = text_height;
}

fn update_retained_line_numbers_for_size(
    state: &mut TextPreviewViewerState,
    digit_count: usize,
    gutter_width: f32,
    scroll_height: f32,
) {
    let scroll_y = editor_scroll_y(&state.editor);
    let line_numbers_need_update = (state.retained_line_number_scroll_y - scroll_y).abs()
        >= f32::EPSILON
        || state.retained_line_number_digit_count != digit_count
        || (state.retained_line_number_gutter_width - gutter_width).abs() >= f32::EPSILON
        || (state.retained_line_number_height - scroll_height).abs() >= f32::EPSILON;
    if !line_numbers_need_update && !state.retained_line_numbers.is_empty() {
        return;
    }

    state.retained_line_numbers = line_number_offsets(&state.editor, scroll_height)
        .into_iter()
        .map(|(line_index, y)| TextPreviewLineNumber {
            y,
            paragraph: line_number_paragraph(line_index + 1, digit_count, gutter_width),
        })
        .collect();
    state.retained_line_number_scroll_y = scroll_y;
    state.retained_line_number_digit_count = digit_count;
    state.retained_line_number_gutter_width = gutter_width;
    state.retained_line_number_height = scroll_height;
}

fn apply_key_binding<Message>(
    binding: text_editor::Binding<Message>,
    editor: &mut PreviewEditor,
    clipboard: &mut dyn Clipboard,
) {
    match binding {
        text_editor::Binding::Copy | text_editor::Binding::Cut => {
            if let Some(selection) = editor.copy() {
                clipboard.write(ClipboardKind::Standard, selection);
            }
        }
        text_editor::Binding::Move(motion) => editor.perform(text_editor::Action::Move(motion)),
        text_editor::Binding::Select(motion) => {
            editor.perform(text_editor::Action::Select(motion));
        }
        text_editor::Binding::SelectWord => editor.perform(text_editor::Action::SelectWord),
        text_editor::Binding::SelectLine => editor.perform(text_editor::Action::SelectLine),
        text_editor::Binding::SelectAll => editor.perform(text_editor::Action::SelectAll),
        text_editor::Binding::Sequence(sequence) => {
            for binding in sequence {
                apply_key_binding(binding, editor, clipboard);
            }
        }
        text_editor::Binding::Unfocus
        | text_editor::Binding::Paste
        | text_editor::Binding::Insert(_)
        | text_editor::Binding::Enter
        | text_editor::Binding::Backspace
        | text_editor::Binding::Delete
        | text_editor::Binding::Custom(_) => {}
    }
}

fn draw_selection_ranges(
    renderer: &mut iced::Renderer,
    editor: &PreviewEditor,
    text_bounds: Rectangle,
    theme: &Theme,
) {
    let Selection::Range(ranges) = editor.selection() else {
        return;
    };
    let translation = text_bounds.position() - Point::ORIGIN;
    for range in ranges
        .into_iter()
        .filter_map(|range| text_bounds.intersection(&(range + translation)))
    {
        renderer.fill_quad(
            renderer::Quad {
                bounds: range,
                ..renderer::Quad::default()
            },
            selection_color(theme),
        );
    }
}

fn draw_visible_text_lines(
    renderer: &mut iced::Renderer,
    state: &TextPreviewViewerState,
    text_bounds: Rectangle,
    theme: &Theme,
    viewport: &Rectangle,
) {
    let Some(clip_bounds) = text_bounds.intersection(viewport) else {
        return;
    };
    let color = text_color(theme);
    for text_line in &state.retained_text_lines {
        renderer.fill_paragraph(
            &text_line.paragraph,
            Point::new(text_bounds.x, text_bounds.y + text_line.y),
            color,
            clip_bounds,
        );
    }
}

fn text_bounds(bounds: Rectangle, document: &TextPreviewDocument) -> Rectangle {
    let gutter_width = text_preview_line_number_gutter_width(document);
    let x = bounds.x + gutter_width + TEXT_PREVIEW_GUTTER_SPACING + TEXT_PREVIEW_DIVIDER_WIDTH;
    Rectangle {
        x,
        y: bounds.y,
        width: (bounds.width - (x - bounds.x)).max(0.0),
        height: bounds.height,
    }
    .shrink(Padding::new(TEXT_PREVIEW_VIEWER_PADDING))
}

fn text_preview_line_number_gutter_width(document: &TextPreviewDocument) -> f32 {
    let digit_width = document.line_number_digit_count() as f32 * TEXT_PREVIEW_GUTTER_DIGIT_WIDTH;
    let padding_width = TEXT_PREVIEW_GUTTER_HORIZONTAL_PADDING * 2.0;

    (digit_width + padding_width).max(TEXT_PREVIEW_GUTTER_MIN_WIDTH)
}

fn text_preview_scroll_lines(delta: mouse::ScrollDelta) -> f32 {
    match delta {
        mouse::ScrollDelta::Lines { y, .. } => {
            if y.abs() > 0.0 {
                y.signum() * -(y.abs() * 4.0).max(1.0)
            } else {
                0.0
            }
        }
        mouse::ScrollDelta::Pixels { y, .. } => -y / 4.0,
    }
}

fn apply_bounded_scroll_lines(
    state: &mut TextPreviewViewerState,
    requested_lines: i32,
    viewport_height: f32,
) -> bool {
    clamp_editor_scroll_to_bounds(state, viewport_height);
    let scroll_lines = bounded_scroll_lines(&state.editor, requested_lines, viewport_height);
    if scroll_lines == 0 {
        return false;
    }

    state.editor.perform(text_editor::Action::Scroll {
        lines: scroll_lines,
    });
    clamp_editor_scroll_to_bounds(state, viewport_height);
    true
}

fn clamp_editor_scroll_to_bounds(state: &mut TextPreviewViewerState, viewport_height: f32) {
    let current_scroll_y = editor_scroll_y(&state.editor);
    if !current_scroll_y.is_finite() {
        return;
    }

    let line_height = state.editor.buffer().metrics().line_height;
    if !line_height.is_finite() || line_height <= 0.0 {
        return;
    }

    let max_scroll_y = max_editor_scroll_y(&state.editor, viewport_height);
    let scroll_lines = if current_scroll_y < 0.0 {
        (current_scroll_y.abs() / line_height).ceil() as i32
    } else if current_scroll_y > max_scroll_y {
        -((current_scroll_y - max_scroll_y) / line_height).ceil() as i32
    } else {
        0
    };

    if scroll_lines != 0 {
        state.editor.perform(text_editor::Action::Scroll {
            lines: scroll_lines,
        });
    }
}

fn bounded_scroll_lines(editor: &PreviewEditor, requested_lines: i32, viewport_height: f32) -> i32 {
    if requested_lines == 0 || !viewport_height.is_finite() || viewport_height <= 0.0 {
        return 0;
    }

    let line_height = editor.buffer().metrics().line_height;
    if !line_height.is_finite() || line_height <= 0.0 {
        return 0;
    }

    let max_scroll_y = max_editor_scroll_y(editor, viewport_height);
    let current_scroll_y = editor_scroll_y(editor).clamp(0.0, max_scroll_y);
    if requested_lines > 0 {
        let available_lines = ((max_scroll_y - current_scroll_y) / line_height).floor() as i32;
        requested_lines.min(available_lines.max(0))
    } else {
        let available_lines = (current_scroll_y / line_height).floor() as i32;
        requested_lines.max(-available_lines.max(0))
    }
}

fn max_editor_scroll_y(editor: &PreviewEditor, viewport_height: f32) -> f32 {
    (editor_content_height(editor) - viewport_height).max(0.0)
}

fn editor_content_height(editor: &PreviewEditor) -> f32 {
    let buffer = editor.buffer();
    let metrics = buffer.metrics();

    buffer
        .lines
        .iter()
        .map(|line| {
            line.layout_opt()
                .map(|layout| {
                    layout
                        .iter()
                        .map(|layout_line| {
                            layout_line.line_height_opt.unwrap_or(metrics.line_height)
                        })
                        .sum::<f32>()
                })
                .unwrap_or(metrics.line_height)
        })
        .sum()
}

fn line_number_bounds(gutter_bounds: Rectangle, y: f32) -> Rectangle {
    Rectangle {
        x: gutter_bounds.x,
        y,
        width: gutter_bounds.width,
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

fn text_line_paragraph(content: &str, width: f32, height: f32) -> PreviewParagraph {
    PreviewParagraph::with_text(text::Text {
        content,
        bounds: Size::new(width, height),
        size: Pixels(TEXT_PREVIEW_TEXT_SIZE),
        line_height: text::LineHeight::Relative(TEXT_PREVIEW_LINE_HEIGHT),
        font: Font::MONOSPACE,
        align_x: text::Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::WordOrGlyph,
    })
}

fn line_number_offsets(editor: &PreviewEditor, scroll_height: f32) -> Vec<(usize, f32)> {
    visible_logical_lines(editor, scroll_height)
        .into_iter()
        .map(|line| (line.line_index, line.y))
        .collect()
}

fn visible_logical_lines(editor: &PreviewEditor, scroll_height: f32) -> Vec<VisibleLogicalLine> {
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
            offsets.push(VisibleLogicalLine {
                line_index,
                y: visible_y,
                height: line_height,
            });
        }
        if visible_y > scroll_height {
            break;
        }

        y += line_height;
    }

    offsets
}

fn editor_scroll_y(editor: &PreviewEditor) -> f32 {
    editor.buffer().scroll().vertical
}

fn is_dark_theme(theme: &Theme) -> bool {
    let background = theme.palette().background;
    background.r * 0.299 + background.g * 0.587 + background.b * 0.114 < 0.5
}

fn viewer_background_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(20, 27, 38)
    } else {
        Color::from_rgb8(250, 251, 253)
    }
}

fn divider_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(62, 76, 101)
    } else {
        Color::from_rgb8(211, 219, 232)
    }
}

fn text_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(236, 244, 255)
    } else {
        Color::from_rgb8(24, 42, 72)
    }
}

fn placeholder_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(137, 146, 159)
    } else {
        Color::from_rgb8(119, 127, 139)
    }
}

fn selection_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgba8(85, 135, 205, 0.42)
    } else {
        Color::from_rgba8(150, 190, 255, 0.62)
    }
}

#[cfg(test)]
mod tests;
