use iced::advanced::{layout, overlay, renderer, widget, Clipboard, Layout, Shell, Widget};
use iced::mouse;
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

const FLOATING_SURFACE_MARGIN: f32 = 18.0;

pub(crate) fn floating_surface<'a, Message>(
    content: impl Into<Element<'a, Message>>,
    floating: Vec<FloatingContent<'a, Message>>,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    Element::new(FloatingSurface {
        content: content.into(),
        floating,
        background_input_policy: BackgroundInputPolicy::Interactive,
        outside_click_dismissal: None,
    })
}

pub(crate) fn modal_floating_surface<'a, Message>(
    content: impl Into<Element<'a, Message>>,
    floating: Vec<FloatingContent<'a, Message>>,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    Element::new(FloatingSurface {
        content: content.into(),
        floating,
        background_input_policy: BackgroundInputPolicy::Blocked,
        outside_click_dismissal: None,
    })
}

pub(crate) fn dismissable_blocking_floating_surface<'a, Message>(
    content: impl Into<Element<'a, Message>>,
    floating: Vec<FloatingContent<'a, Message>>,
    dismiss_message: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    Element::new(FloatingSurface {
        content: content.into(),
        floating,
        background_input_policy: BackgroundInputPolicy::Blocked,
        outside_click_dismissal: Some(OutsideClickDismissal {
            message: dismiss_message,
            clicked_event_flow: DismissedClickFlow::Capture,
        }),
    })
}

pub(crate) fn pass_through_dismissable_floating_surface<'a, Message>(
    content: impl Into<Element<'a, Message>>,
    floating: Vec<FloatingContent<'a, Message>>,
    dismiss_message: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    Element::new(FloatingSurface {
        content: content.into(),
        floating,
        background_input_policy: BackgroundInputPolicy::Interactive,
        outside_click_dismissal: Some(OutsideClickDismissal {
            message: dismiss_message,
            clicked_event_flow: DismissedClickFlow::Continue,
        }),
    })
}

#[derive(Clone)]
struct OutsideClickDismissal<Message> {
    message: Message,
    clicked_event_flow: DismissedClickFlow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DismissedClickFlow {
    Capture,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackgroundInputPolicy {
    Interactive,
    Blocked,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FloatingPlacement {
    Center,
    At(Point),
    Free(Point),
    BottomLeft {
        left: f32,
        bottom: f32,
    },
    BottomRightInArea {
        area_width: f32,
        right: f32,
        bottom: f32,
    },
}

pub(crate) struct FloatingContent<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    pub(crate) element: Element<'a, Message, Theme, Renderer>,
    pub(crate) placement: FloatingPlacement,
}

struct FloatingSurface<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    content: Element<'a, Message, Theme, Renderer>,
    floating: Vec<FloatingContent<'a, Message, Theme, Renderer>>,
    background_input_policy: BackgroundInputPolicy,
    outside_click_dismissal: Option<OutsideClickDismissal<Message>>,
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for FloatingSurface<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn children(&self) -> Vec<widget::Tree> {
        let mut children = vec![widget::Tree::new(&self.content)];

        for floating in &self.floating {
            children.push(widget::Tree::new(&floating.element));
        }

        children
    }

    fn diff(&self, tree: &mut widget::Tree) {
        let mut children = Vec::with_capacity(1 + self.floating.len());
        children.push(self.content.as_widget());

        for floating in &self.floating {
            children.push(floating.element.as_widget());
        }

        tree.diff_children(&children);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation<Message>,
    ) {
        self.content
            .as_widget()
            .operate(&mut tree.children[0], layout, renderer, operation);

        for (index, floating) in self.floating.iter().enumerate() {
            if let Some(floating_tree) = tree.children.get_mut(index + 1) {
                floating
                    .element
                    .as_widget()
                    .operate(floating_tree, layout, renderer, operation);
            }
        }
    }

    fn on_event(
        &mut self,
        tree: &mut widget::Tree,
        event: Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> iced::event::Status {
        if self.is_outside_dismiss_click(&event, &mut tree.children, layout, cursor, renderer) {
            if let Some(dismissal) = self.outside_click_dismissal.clone() {
                shell.publish(dismissal.message);
                if dismissal.clicked_event_flow == DismissedClickFlow::Capture {
                    return iced::event::Status::Captured;
                }
            }
        }

        if self.background_input_policy == BackgroundInputPolicy::Blocked {
            return iced::event::Status::Captured;
        }

        self.content.as_widget_mut().on_event(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            self.background_cursor(cursor),
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
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
            self.background_cursor(cursor),
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let mut overlays = Vec::new();
        let mut children = tree.children.iter_mut();

        if let Some(content_tree) = children.next() {
            if let Some(content_overlay) =
                self.content
                    .as_widget_mut()
                    .overlay(content_tree, layout, renderer, translation)
            {
                overlays.push(content_overlay);
            }
        }

        for (floating, floating_tree) in self.floating.iter_mut().zip(children) {
            overlays.push(overlay::Element::new(Box::new(FloatingOverlay {
                floating: &mut floating.element,
                placement: floating.placement,
                state: floating_tree,
            })));
        }

        (!overlays.is_empty()).then(|| overlay::Group::with_children(overlays).overlay())
    }
}

impl<'a, Message, Theme, Renderer> FloatingSurface<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn is_outside_dismiss_click(
        &self,
        event: &Event,
        children: &mut [widget::Tree],
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> bool {
        if self.outside_click_dismissal.is_none()
            || !matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            )
        {
            return false;
        }
        let Some(position) = cursor.position() else {
            return false;
        };

        let surface_size = Size::new(layout.bounds().width, layout.bounds().height);
        let mut checked_any_floating = false;
        for (floating, tree) in self.floating.iter().zip(children.iter_mut().skip(1)) {
            checked_any_floating = true;
            let limits = layout::Limits::new(
                Size::ZERO,
                floating_max_size(floating.placement, surface_size),
            );
            let node = floating.element.as_widget().layout(tree, renderer, &limits);
            if floating_bounds(floating.placement, node.size(), surface_size).contains(position) {
                return false;
            }
        }

        checked_any_floating
    }

    fn background_cursor(&self, cursor: mouse::Cursor) -> mouse::Cursor {
        match self.background_input_policy {
            BackgroundInputPolicy::Interactive => cursor,
            BackgroundInputPolicy::Blocked => mouse::Cursor::Unavailable,
        }
    }
}

struct FloatingOverlay<'a, 'b, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    floating: &'b mut Element<'a, Message, Theme, Renderer>,
    placement: FloatingPlacement,
    state: &'b mut widget::Tree,
}

impl<'a, 'b, Message, Theme, Renderer> overlay::Overlay<Message, Theme, Renderer>
    for FloatingOverlay<'a, 'b, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let max_size = floating_max_size(self.placement, bounds);
        let limits = layout::Limits::new(Size::ZERO, max_size);
        let node = self
            .floating
            .as_widget()
            .layout(self.state, renderer, &limits);
        let size = node.size();
        node.move_to(floating_position(self.placement, size, bounds))
    }

    fn on_event(
        &mut self,
        event: Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) -> iced::event::Status {
        let bounds = layout.bounds();

        self.floating.as_widget_mut().on_event(
            self.state, event, layout, cursor, renderer, clipboard, shell, &bounds,
        )
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.floating
            .as_widget()
            .mouse_interaction(self.state, layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.floating.as_widget().draw(
            self.state,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &Rectangle::with_size(Size::INFINITY),
        );
    }

    fn is_over(&self, layout: Layout<'_>, _renderer: &Renderer, cursor_position: Point) -> bool {
        layout.bounds().contains(cursor_position)
    }
}

fn floating_max_size(placement: FloatingPlacement, bounds: Size) -> Size {
    match placement {
        FloatingPlacement::Free(_) => bounds,
        FloatingPlacement::Center
        | FloatingPlacement::At(_)
        | FloatingPlacement::BottomLeft { .. }
        | FloatingPlacement::BottomRightInArea { .. } => Size::new(
            (bounds.width - FLOATING_SURFACE_MARGIN * 2.0).max(0.0),
            (bounds.height - FLOATING_SURFACE_MARGIN * 2.0).max(0.0),
        ),
    }
}

fn floating_bounds(placement: FloatingPlacement, size: Size, surface: Size) -> Rectangle {
    Rectangle::new(floating_position(placement, size, surface), size)
}

fn floating_position(placement: FloatingPlacement, size: Size, surface: Size) -> Point {
    let desired = match placement {
        FloatingPlacement::Center => Point::new(
            (surface.width - size.width) / 2.0,
            (surface.height - size.height) / 2.0,
        ),
        FloatingPlacement::At(position) | FloatingPlacement::Free(position) => position,
        FloatingPlacement::BottomLeft { left, bottom } => {
            Point::new(left, surface.height - bottom - size.height)
        }
        FloatingPlacement::BottomRightInArea {
            area_width,
            right,
            bottom,
        } => Point::new(
            area_width - right - size.width,
            surface.height - bottom - size.height,
        ),
    };
    if matches!(placement, FloatingPlacement::Free(_)) {
        return desired;
    }

    let max_x = (surface.width - size.width - FLOATING_SURFACE_MARGIN).max(FLOATING_SURFACE_MARGIN);
    let max_y =
        (surface.height - size.height - FLOATING_SURFACE_MARGIN).max(FLOATING_SURFACE_MARGIN);
    Point::new(
        desired.x.max(FLOATING_SURFACE_MARGIN).min(max_x),
        desired.y.max(FLOATING_SURFACE_MARGIN).min(max_y),
    )
}
