use iced::advanced::text::Paragraph;
use iced::advanced::{layout, renderer, text, widget, Layout, Widget};
use iced::{alignment, Element, Length, Pixels, Point, Rectangle, Size};

mod tooltip;

pub(crate) use tooltip::measured_middle_ellipsized_wrapped_text_with_tooltip;

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

#[derive(Debug, Clone, Copy)]
enum MiddleEllipsisKind {
    GeneralText,
    FileName,
}

#[derive(Debug, Clone)]
struct MeasuredMiddleEllipsizedText {
    content: String,
    size: Pixels,
    width: Length,
    height: Length,
    line_height: text::LineHeight,
    wrapping: text::Wrapping,
    align_x: text::Alignment,
    ellipsis_kind: MiddleEllipsisKind,
}

impl MeasuredMiddleEllipsizedText {
    fn new(content: impl Into<String>) -> Self {
        Self::with_ellipsis_kind(content, MiddleEllipsisKind::GeneralText)
    }

    fn file_name(content: impl Into<String>) -> Self {
        Self::with_ellipsis_kind(content, MiddleEllipsisKind::FileName)
    }

    fn with_ellipsis_kind(content: impl Into<String>, ellipsis_kind: MiddleEllipsisKind) -> Self {
        Self {
            content: content.into(),
            size: Pixels(16.0),
            width: Length::Shrink,
            height: Length::Shrink,
            line_height: text::LineHeight::default(),
            wrapping: text::Wrapping::None,
            align_x: text::Alignment::Left,
            ellipsis_kind,
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

    fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    fn line_height(mut self, line_height: text::LineHeight) -> Self {
        self.line_height = line_height;
        self
    }

    fn wrapping(mut self, wrapping: text::Wrapping) -> Self {
        self.wrapping = wrapping;
        self
    }

    fn align_x(mut self, align_x: text::Alignment) -> Self {
        self.align_x = align_x;
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

fn displayed_content_is_ellipsized(content: &str, displayed_content: &str) -> bool {
    displayed_content != content
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
            let line_height = self.line_height;
            let shaping = shaping_for_content(&self.content);
            let display_bounds = Size::new(bounds.width, bounds.height);
            let displayed_content = fit_middle_ellipsized_text::<Renderer>(
                &self.content,
                bounds.width,
                bounds.height,
                self.size,
                line_height,
                font,
                shaping,
                self.wrapping,
                self.ellipsis_kind,
            );

            text_state.displayed_content = displayed_content;
            text_state.paragraph = Renderer::Paragraph::with_text(text::Text {
                content: text_state.displayed_content.as_str(),
                bounds: display_bounds,
                size: self.size,
                line_height,
                font,
                align_x: self.align_x,
                align_y: alignment::Vertical::Top,
                shaping,
                wrapping: self.wrapping,
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
            paragraph_draw_anchor(bounds, &text_state.paragraph),
            style.text_color,
            *viewport,
        );
    }
}

fn paragraph_draw_anchor<Paragraph>(bounds: Rectangle, paragraph: &Paragraph) -> Point
where
    Paragraph: text::Paragraph,
{
    bounds.anchor(
        paragraph.min_bounds(),
        paragraph.align_x(),
        paragraph.align_y(),
    )
}

fn fit_middle_ellipsized_text<Renderer>(
    content: &str,
    available_width: f32,
    available_height: f32,
    size: Pixels,
    line_height: text::LineHeight,
    font: Renderer::Font,
    shaping: text::Shaping,
    wrapping: text::Wrapping,
    ellipsis_kind: MiddleEllipsisKind,
) -> String
where
    Renderer: text::Renderer,
{
    if content.is_empty() || available_width <= 0.0 || available_height <= 0.0 {
        return String::new();
    }

    fit_middle_ellipsized_by(content, ellipsis_kind, |candidate| {
        measured_text_fits::<Renderer>(
            candidate,
            available_width,
            available_height,
            size,
            line_height,
            font,
            shaping,
            wrapping,
        )
    })
}

fn fit_middle_ellipsized_by(
    content: &str,
    ellipsis_kind: MiddleEllipsisKind,
    mut fits: impl FnMut(&str) -> bool,
) -> String {
    if fits(content) {
        return content.to_owned();
    }

    if !fits(ELLIPSIS_MARKER) {
        return String::new();
    }

    let content_char_count = content.chars().count();
    let mut lower = 0usize;
    let mut upper = content_char_count.saturating_sub(1);
    let mut best = ELLIPSIS_MARKER.to_owned();

    while lower <= upper {
        let visible_chars = lower + (upper - lower) / 2;
        let candidate = ellipsis_kind.candidate(content, visible_chars);

        if fits(&candidate) {
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

#[allow(clippy::too_many_arguments)]
fn measured_text_fits<Renderer>(
    content: &str,
    available_width: f32,
    available_height: f32,
    size: Pixels,
    line_height: text::LineHeight,
    font: Renderer::Font,
    shaping: text::Shaping,
    wrapping: text::Wrapping,
) -> bool
where
    Renderer: text::Renderer,
{
    let paragraph = Renderer::Paragraph::with_text(text::Text {
        content,
        bounds: Size::new(available_width, TEXT_MEASUREMENT_HEIGHT),
        size,
        line_height,
        font,
        align_x: text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping,
        wrapping,
    });
    let bounds = paragraph.min_bounds();

    bounds.width <= available_width && bounds.height <= available_height
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

pub(crate) fn measured_text_natural_width<Renderer>(
    renderer: &Renderer,
    content: &str,
    size: u32,
) -> f32
where
    Renderer: text::Renderer,
{
    measured_text_width::<Renderer>(
        content,
        Pixels(size as f32),
        text::LineHeight::default(),
        renderer.default_font(),
        shaping_for_content(content),
    )
}

fn shaping_for_content(content: &str) -> text::Shaping {
    if content.is_ascii() {
        text::Shaping::Basic
    } else {
        text::Shaping::Advanced
    }
}

impl MiddleEllipsisKind {
    fn candidate(self, content: &str, visible_chars: usize) -> String {
        match self {
            Self::GeneralText => middle_ellipsized_candidate(content, visible_chars),
            Self::FileName => middle_ellipsized_filename_candidate(content, visible_chars),
        }
    }
}

fn middle_ellipsized_candidate(content: &str, visible_chars: usize) -> String {
    middle_ellipsized_candidate_with_minimum_suffix(content, visible_chars, 0)
}

fn middle_ellipsized_filename_candidate(content: &str, visible_chars: usize) -> String {
    let extension_chars = content
        .rsplit_once('.')
        .filter(|(stem, extension)| !stem.is_empty() && !extension.is_empty())
        .map_or(0, |(_, extension)| extension.chars().count() + 1);

    middle_ellipsized_candidate_with_minimum_suffix(content, visible_chars, extension_chars)
}

fn middle_ellipsized_candidate_with_minimum_suffix(
    content: &str,
    visible_chars: usize,
    minimum_suffix_chars: usize,
) -> String {
    let char_count = content.chars().count();
    if visible_chars >= char_count {
        return content.to_owned();
    }

    let end_chars = (visible_chars / 2)
        .max(minimum_suffix_chars)
        .min(visible_chars.saturating_sub(1));
    let start_chars = visible_chars - end_chars;
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
    use super::{
        displayed_content_is_ellipsized, fit_middle_ellipsized_by, fit_middle_ellipsized_text,
        measured_text_fits, middle_ellipsized_candidate, middle_ellipsized_filename_candidate,
        shaping_for_content, MeasuredMiddleEllipsizedText, MeasuredMiddleEllipsizedTextState,
        MiddleEllipsisKind, ELLIPSIS_MARKER,
    };
    use crate::icon_grid_geometry::{
        ICON_GRID_LABEL_HEIGHT, ICON_GRID_LABEL_LINE_HEIGHT_PX, ICON_GRID_LABEL_SIZE,
    };
    use iced::advanced::text::{self, Paragraph as _};
    use iced::advanced::{image, layout, mouse, renderer, widget::Tree, Layout};
    use iced::{
        Background, Color, Element, Font, Length, Pixels, Point, Rectangle, Size, Theme,
        Transformation,
    };

    #[derive(Default)]
    struct RecordingRenderer {
        paragraph_positions: Vec<Point>,
    }

    impl iced::advanced::Renderer for RecordingRenderer {
        fn start_layer(&mut self, _bounds: Rectangle) {}

        fn end_layer(&mut self) {}

        fn start_transformation(&mut self, _transformation: Transformation) {}

        fn end_transformation(&mut self) {}

        fn fill_quad(&mut self, _quad: renderer::Quad, _background: impl Into<Background>) {}

        fn reset(&mut self, _new_bounds: Rectangle) {}

        fn allocate_image(
            &mut self,
            _handle: &image::Handle,
            _callback: impl FnOnce(Result<image::Allocation, image::Error>) + Send + 'static,
        ) {
            panic!("measured text draw must not allocate images");
        }
    }

    impl text::Renderer for RecordingRenderer {
        type Font = Font;
        type Paragraph = <iced::Renderer as text::Renderer>::Paragraph;
        type Editor = <iced::Renderer as text::Renderer>::Editor;

        const ICON_FONT: Font = <iced::Renderer as text::Renderer>::ICON_FONT;
        const CHECKMARK_ICON: char = <iced::Renderer as text::Renderer>::CHECKMARK_ICON;
        const ARROW_DOWN_ICON: char = <iced::Renderer as text::Renderer>::ARROW_DOWN_ICON;
        const SCROLL_UP_ICON: char = <iced::Renderer as text::Renderer>::SCROLL_UP_ICON;
        const SCROLL_DOWN_ICON: char = <iced::Renderer as text::Renderer>::SCROLL_DOWN_ICON;
        const SCROLL_LEFT_ICON: char = <iced::Renderer as text::Renderer>::SCROLL_LEFT_ICON;
        const SCROLL_RIGHT_ICON: char = <iced::Renderer as text::Renderer>::SCROLL_RIGHT_ICON;
        const ICED_LOGO: char = <iced::Renderer as text::Renderer>::ICED_LOGO;

        fn default_font(&self) -> Font {
            Font::default()
        }

        fn default_size(&self) -> Pixels {
            Pixels(16.0)
        }

        fn fill_paragraph(
            &mut self,
            _paragraph: &Self::Paragraph,
            position: Point,
            _color: Color,
            _clip_bounds: Rectangle,
        ) {
            self.paragraph_positions.push(position);
        }

        fn fill_editor(
            &mut self,
            _editor: &Self::Editor,
            _position: Point,
            _color: Color,
            _clip_bounds: Rectangle,
        ) {
            panic!("measured text draw must not fill an editor");
        }

        fn fill_text(
            &mut self,
            _text: text::Text<String, Font>,
            _position: Point,
            _color: Color,
            _clip_bounds: Rectangle,
        ) {
            panic!("measured text draw must not fill uncached text");
        }
    }

    fn recorded_widget_draw(align_x: text::Alignment) -> (Rectangle, Size, Point) {
        let widget = MeasuredMiddleEllipsizedText::new("short.txt")
            .size(ICON_GRID_LABEL_SIZE)
            .line_height(text::LineHeight::Absolute(Pixels(
                ICON_GRID_LABEL_LINE_HEIGHT_PX,
            )))
            .wrapping(text::Wrapping::WordOrGlyph)
            .width(Length::Fill)
            .height(Length::Fixed(ICON_GRID_LABEL_HEIGHT))
            .align_x(align_x);
        let mut element: Element<'_, (), Theme, RecordingRenderer> = widget.into();
        let mut tree = Tree::new(element.as_widget());
        let mut renderer = RecordingRenderer::default();
        let limits = layout::Limits::new(Size::ZERO, Size::new(128.0, ICON_GRID_LABEL_HEIGHT));
        let node = element
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits)
            .move_to(Point::new(10.0, 20.0));
        let bounds = node.bounds();
        let viewport = bounds;

        element.as_widget().draw(
            &tree,
            &mut renderer,
            &Theme::Light,
            &renderer::Style::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &viewport,
        );

        let paragraph = &tree
            .state
            .downcast_ref::<MeasuredMiddleEllipsizedTextState<
                <RecordingRenderer as text::Renderer>::Paragraph,
            >>()
            .paragraph;
        assert_eq!(renderer.paragraph_positions.len(), 1);

        (
            bounds,
            paragraph.min_bounds(),
            renderer.paragraph_positions[0],
        )
    }

    #[test]
    fn widget_draw_passes_the_aligned_anchor_to_fill_paragraph() {
        let (center_bounds, center_min_bounds, center_position) =
            recorded_widget_draw(text::Alignment::Center);
        assert_eq!(
            center_position,
            center_bounds.anchor(
                center_min_bounds,
                text::Alignment::Center,
                iced::alignment::Vertical::Top,
            )
        );
        assert!(center_position.x > center_bounds.x);

        let (left_bounds, _, left_position) = recorded_widget_draw(text::Alignment::Left);
        assert_eq!(left_position, left_bounds.position());
    }

    #[test]
    fn tooltip_eligibility_matches_actual_displayed_content() {
        assert!(!displayed_content_is_ellipsized("short.txt", "short.txt"));
        assert!(displayed_content_is_ellipsized(
            "very-long-file-name.txt",
            "very...name.txt"
        ));
    }

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

    #[test]
    fn filename_candidate_preserves_the_complete_extension() {
        let candidate = middle_ellipsized_filename_candidate("archive-name.longextension", 18);

        assert!(candidate.starts_with("arch"));
        assert!(candidate.ends_with(".longextension"));
        assert!(candidate.contains(ELLIPSIS_MARKER));
    }

    #[test]
    fn fixed_three_line_fit_keeps_the_longest_measured_candidate() {
        let content = "abcdefghijklmnop.rs";
        let three_line_capacity = 13;
        let displayed =
            fit_middle_ellipsized_by(content, MiddleEllipsisKind::FileName, |candidate| {
                candidate.chars().count() <= three_line_capacity
            });

        assert_eq!(displayed.chars().count(), three_line_capacity);
        assert!(displayed.starts_with("abcde"));
        assert!(displayed.ends_with("op.rs"));
        assert!(displayed.contains(ELLIPSIS_MARKER));
    }

    #[test]
    fn fixed_three_line_fit_keeps_content_that_renderer_reports_as_fitting() {
        let content = "three-line-name.rs";

        assert_eq!(
            fit_middle_ellipsized_by(content, MiddleEllipsisKind::GeneralText, |candidate| {
                candidate == content
            }),
            content
        );
    }

    #[test]
    fn wrapped_fit_uses_renderer_measurement_to_stay_within_three_lines() {
        let content = "very-long-file-name-with-many-segments-that-needs-four-lines.rs";
        let width = 96.0;
        let size = Pixels(ICON_GRID_LABEL_SIZE);
        let line_height = text::LineHeight::Absolute(Pixels(ICON_GRID_LABEL_LINE_HEIGHT_PX));
        let height = ICON_GRID_LABEL_HEIGHT;
        let font = iced::Font::default();
        let shaping = shaping_for_content(content);
        let wrapping = text::Wrapping::WordOrGlyph;

        assert!(!measured_text_fits::<iced::Renderer>(
            content,
            width,
            height,
            size,
            line_height,
            font,
            shaping,
            wrapping,
        ));

        let displayed = fit_middle_ellipsized_text::<iced::Renderer>(
            content,
            width,
            height,
            size,
            line_height,
            font,
            shaping,
            wrapping,
            MiddleEllipsisKind::FileName,
        );

        assert!(displayed.starts_with('v'));
        assert!(displayed.contains(ELLIPSIS_MARKER));
        assert!(displayed.ends_with(".rs"));
        assert!(displayed_content_is_ellipsized(content, &displayed));
        assert!(measured_text_fits::<iced::Renderer>(
            &displayed,
            width,
            height,
            size,
            line_height,
            font,
            shaping,
            wrapping,
        ));
    }
}
