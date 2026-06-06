use iced::advanced::text::Paragraph;
use iced::advanced::{layout, renderer, text, widget, Layout, Widget};
use iced::{alignment, Element, Length, Pixels, Rectangle, Size};

const ELLIPSIS_MARKER: &str = "...";
const TEXT_MEASUREMENT_WIDTH: f32 = 100_000.0;
const TEXT_MEASUREMENT_HEIGHT: f32 = 10_000.0;

// Finder 的多栏列表按最终像素宽度截断；这里必须等布局拿到 renderer 后再决定是否省略。
pub(crate) fn measured_middle_ellipsized_text<'a, Message>(
    content: impl Into<String>,
    size: u32,
) -> Element<'a, Message>
where
    Message: 'a,
{
    Element::new(
        MeasuredMiddleEllipsizedText::new(content)
            .size(size)
            .width(Length::Fill),
    )
}

#[derive(Debug, Clone)]
struct MeasuredMiddleEllipsizedText {
    content: String,
    size: Pixels,
    width: Length,
    height: Length,
}

impl MeasuredMiddleEllipsizedText {
    fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            size: Pixels(16.0),
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }

    fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.size = size.into();
        self
    }

    fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

struct MeasuredMiddleEllipsizedTextState<Paragraph> {
    paragraph: Paragraph,
    displayed_content: String,
}

impl<Paragraph: Default> Default for MeasuredMiddleEllipsizedTextState<Paragraph> {
    fn default() -> Self {
        Self {
            paragraph: Paragraph::default(),
            displayed_content: String::new(),
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for MeasuredMiddleEllipsizedText
where
    Renderer: text::Renderer,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<MeasuredMiddleEllipsizedTextState<Renderer::Paragraph>>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(
            MeasuredMiddleEllipsizedTextState::<Renderer::Paragraph>::default(),
        )
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let text_state = tree
            .state
            .downcast_mut::<MeasuredMiddleEllipsizedTextState<Renderer::Paragraph>>();

        layout::sized(limits, self.width, self.height, |limits| {
            let bounds = limits.max();
            let font = renderer.default_font();
            let line_height = text::LineHeight::default();
            let shaping = shaping_for_content(&self.content);
            let display_bounds = Size::new(bounds.width, bounds.height);
            let displayed_content = fit_middle_ellipsized_text::<Renderer>(
                &self.content,
                bounds.width,
                self.size,
                line_height,
                font,
                shaping,
            );

            text_state.displayed_content = displayed_content;
            text_state.paragraph = Renderer::Paragraph::with_text(text::Text {
                content: text_state.displayed_content.as_str(),
                bounds: display_bounds,
                size: self.size,
                line_height,
                font,
                align_x: text::Alignment::Left,
                align_y: alignment::Vertical::Top,
                shaping,
                wrapping: text::Wrapping::None,
            });

            text_state.paragraph.min_bounds()
        })
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor_position: iced::mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let text_state = tree
            .state
            .downcast_ref::<MeasuredMiddleEllipsizedTextState<Renderer::Paragraph>>();
        let bounds = layout.bounds();

        renderer.fill_paragraph(
            &text_state.paragraph,
            bounds.position(),
            style.text_color,
            *viewport,
        );
    }
}

fn fit_middle_ellipsized_text<Renderer>(
    content: &str,
    available_width: f32,
    size: Pixels,
    line_height: text::LineHeight,
    font: Renderer::Font,
    shaping: text::Shaping,
) -> String
where
    Renderer: text::Renderer,
{
    if content.is_empty() || available_width <= 0.0 {
        return String::new();
    }

    if measured_text_width::<Renderer>(content, size, line_height, font, shaping) <= available_width
    {
        return content.to_owned();
    }

    if measured_text_width::<Renderer>(ELLIPSIS_MARKER, size, line_height, font, shaping)
        > available_width
    {
        return String::new();
    }

    let content_char_count = content.chars().count();
    let mut lower = 0usize;
    let mut upper = content_char_count.saturating_sub(1);
    let mut best = ELLIPSIS_MARKER.to_owned();

    while lower <= upper {
        let visible_chars = lower + (upper - lower) / 2;
        let candidate = middle_ellipsized_candidate(content, visible_chars);

        if measured_text_width::<Renderer>(&candidate, size, line_height, font, shaping)
            <= available_width
        {
            best = candidate;
            lower = visible_chars + 1;
        } else if visible_chars == 0 {
            break;
        } else {
            upper = visible_chars - 1;
        }
    }

    best
}

fn measured_text_width<Renderer>(
    content: &str,
    size: Pixels,
    line_height: text::LineHeight,
    font: Renderer::Font,
    shaping: text::Shaping,
) -> f32
where
    Renderer: text::Renderer,
{
    let paragraph = Renderer::Paragraph::with_text(text::Text {
        content,
        bounds: Size::new(TEXT_MEASUREMENT_WIDTH, TEXT_MEASUREMENT_HEIGHT),
        size,
        line_height,
        font,
        align_x: text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping,
        wrapping: text::Wrapping::None,
    });

    paragraph.min_width()
}

fn shaping_for_content(content: &str) -> text::Shaping {
    if content.is_ascii() {
        text::Shaping::Basic
    } else {
        text::Shaping::Advanced
    }
}

fn middle_ellipsized_candidate(content: &str, visible_chars: usize) -> String {
    let char_count = content.chars().count();
    if visible_chars >= char_count {
        return content.to_owned();
    }

    let start_chars = visible_chars.div_ceil(2);
    let end_chars = visible_chars / 2;
    let mut candidate =
        String::with_capacity(content.len().min(visible_chars + ELLIPSIS_MARKER.len()));

    candidate.extend(content.chars().take(start_chars));
    candidate.push_str(ELLIPSIS_MARKER);
    candidate.extend(
        content
            .chars()
            .rev()
            .take(end_chars)
            .collect::<Vec<_>>()
            .into_iter()
            .rev(),
    );
    candidate
}

impl<'a, Message, Theme, Renderer> From<MeasuredMiddleEllipsizedText>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: text::Renderer + 'a,
{
    fn from(text: MeasuredMiddleEllipsizedText) -> Self {
        Element::new(text)
    }
}

#[cfg(test)]
mod tests {
    use super::middle_ellipsized_candidate;

    #[test]
    fn candidate_preserves_start_and_end() {
        assert_eq!(
            middle_ellipsized_candidate("very-long-name.rs", 8),
            "very...e.rs"
        );
    }

    #[test]
    fn candidate_keeps_whole_content_when_visible_chars_fit() {
        assert_eq!(middle_ellipsized_candidate("short", 5), "short");
    }

    #[test]
    fn candidate_handles_multibyte_characters() {
        assert_eq!(
            middle_ellipsized_candidate("项目文件夹名称", 4),
            "项目...名称"
        );
    }
}
