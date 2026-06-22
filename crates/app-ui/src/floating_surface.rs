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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FloatingPointerTarget {
    FloatingBounds,
    Background,
    CursorUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FloatingInputDecision<Message> {
    dismiss_message: Option<Message>,
    background_update: BackgroundUpdateDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackgroundUpdateDecision {
    Update,
    Stop,
    Capture,
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
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);

        for (index, floating) in self.floating.iter_mut().enumerate() {
            if let Some(floating_tree) = tree.children.get_mut(index + 1) {
                floating.element.as_widget_mut().operate(
                    floating_tree,
                    layout,
                    renderer,
                    operation,
                );
            }
        }
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let floating_pointer_target =
            self.floating_pointer_target(&mut tree.children, layout, cursor, renderer);
        let input_decision = decide_floating_input(
            self.background_input_policy,
            self.outside_click_dismissal.as_ref(),
            floating_pointer_target,
            is_mouse_event(event),
            is_left_mouse_press(event),
        );
        if let Some(message) = input_decision.dismiss_message {
            shell.publish(message);
        }
        match input_decision.background_update {
            BackgroundUpdateDecision::Update => {}
            BackgroundUpdateDecision::Stop => return,
            BackgroundUpdateDecision::Capture => {
                shell.capture_event();
                return;
            }
        }

        self.content.as_widget_mut().update(
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
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let mut overlays = Vec::new();
        let mut children = tree.children.iter_mut();

        if let Some(content_tree) = children.next() {
            if let Some(content_overlay) = self.content.as_widget_mut().overlay(
                content_tree,
                layout,
                renderer,
                viewport,
                translation,
            ) {
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
    fn floating_pointer_target(
        &mut self,
        children: &mut [widget::Tree],
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> FloatingPointerTarget {
        let Some(position) = cursor.position() else {
            return FloatingPointerTarget::CursorUnavailable;
        };

        let surface_size = Size::new(layout.bounds().width, layout.bounds().height);
        let mut checked_any_floating = false;
        for (floating, tree) in self.floating.iter_mut().zip(children.iter_mut().skip(1)) {
            checked_any_floating = true;
            let limits = layout::Limits::new(
                Size::ZERO,
                floating_max_size(floating.placement, surface_size),
            );
            let node = floating
                .element
                .as_widget_mut()
                .layout(tree, renderer, &limits);
            if floating_bounds(floating.placement, node.size(), surface_size).contains(position) {
                return FloatingPointerTarget::FloatingBounds;
            }
        }

        if checked_any_floating {
            FloatingPointerTarget::Background
        } else {
            FloatingPointerTarget::CursorUnavailable
        }
    }

    fn background_cursor(&self, cursor: mouse::Cursor) -> mouse::Cursor {
        match self.background_input_policy {
            BackgroundInputPolicy::Interactive => cursor,
            BackgroundInputPolicy::Blocked => mouse::Cursor::Unavailable,
        }
    }
}

fn decide_floating_input<Message: Clone>(
    background_input_policy: BackgroundInputPolicy,
    outside_click_dismissal: Option<&OutsideClickDismissal<Message>>,
    pointer_target: FloatingPointerTarget,
    is_mouse_event: bool,
    is_left_mouse_press: bool,
) -> FloatingInputDecision<Message> {
    if pointer_target == FloatingPointerTarget::FloatingBounds
        && (background_input_policy == BackgroundInputPolicy::Blocked
            || outside_click_dismissal.is_some())
    {
        return FloatingInputDecision {
            dismiss_message: None,
            background_update: if is_mouse_event {
                BackgroundUpdateDecision::Capture
            } else {
                BackgroundUpdateDecision::Stop
            },
        };
    }

    if is_left_mouse_press && pointer_target == FloatingPointerTarget::Background {
        if let Some(dismissal) = outside_click_dismissal {
            return FloatingInputDecision {
                dismiss_message: Some(dismissal.message.clone()),
                background_update: match dismissal.clicked_event_flow {
                    DismissedClickFlow::Capture => BackgroundUpdateDecision::Capture,
                    DismissedClickFlow::Continue => BackgroundUpdateDecision::Update,
                },
            };
        }
    }

    if background_input_policy == BackgroundInputPolicy::Blocked {
        return FloatingInputDecision {
            dismiss_message: None,
            background_update: if is_mouse_event {
                BackgroundUpdateDecision::Capture
            } else {
                BackgroundUpdateDecision::Stop
            },
        };
    }

    FloatingInputDecision {
        dismiss_message: None,
        background_update: BackgroundUpdateDecision::Update,
    }
}

fn is_left_mouse_press(event: &Event) -> bool {
    matches!(
        event,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
    )
}

fn is_mouse_event(event: &Event) -> bool {
    matches!(event, Event::Mouse(_))
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
            .as_widget_mut()
            .layout(self.state, renderer, &limits);
        let size = node.size();
        node.move_to(floating_position(self.placement, size, bounds))
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let bounds = layout.bounds();

        self.floating.as_widget_mut().update(
            self.state, event, layout, cursor, renderer, clipboard, shell, &bounds,
        )
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        self.floating
            .as_widget()
            .mouse_interaction(self.state, layout, cursor, &bounds, renderer)
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let bounds = layout.bounds();
        self.floating
            .as_widget()
            .draw(self.state, renderer, theme, style, layout, cursor, &bounds);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum TestMessage {
        Dismiss,
    }

    fn dismissal(clicked_event_flow: DismissedClickFlow) -> OutsideClickDismissal<TestMessage> {
        OutsideClickDismissal {
            message: TestMessage::Dismiss,
            clicked_event_flow,
        }
    }

    #[test]
    fn modal_inside_floating_bounds_stops_background_without_dismissal() {
        let decision = decide_floating_input::<TestMessage>(
            BackgroundInputPolicy::Blocked,
            None,
            FloatingPointerTarget::FloatingBounds,
            true,
            true,
        );

        assert_eq!(
            decision,
            FloatingInputDecision {
                dismiss_message: None,
                background_update: BackgroundUpdateDecision::Capture
            }
        );
    }

    #[test]
    fn dismissible_inside_floating_bounds_stops_background_without_dismissal() {
        let dismissal = dismissal(DismissedClickFlow::Capture);

        let decision = decide_floating_input(
            BackgroundInputPolicy::Blocked,
            Some(&dismissal),
            FloatingPointerTarget::FloatingBounds,
            true,
            true,
        );

        assert_eq!(
            decision,
            FloatingInputDecision {
                dismiss_message: None,
                background_update: BackgroundUpdateDecision::Capture
            }
        );
    }

    #[test]
    fn modal_outside_left_click_captures_without_dismissal() {
        let decision = decide_floating_input::<TestMessage>(
            BackgroundInputPolicy::Blocked,
            None,
            FloatingPointerTarget::Background,
            true,
            true,
        );

        assert_eq!(
            decision,
            FloatingInputDecision {
                dismiss_message: None,
                background_update: BackgroundUpdateDecision::Capture
            }
        );
    }

    #[test]
    fn blocking_dismissible_outside_left_click_dismisses_and_captures() {
        let dismissal = dismissal(DismissedClickFlow::Capture);

        let decision = decide_floating_input(
            BackgroundInputPolicy::Blocked,
            Some(&dismissal),
            FloatingPointerTarget::Background,
            true,
            true,
        );

        assert_eq!(
            decision,
            FloatingInputDecision {
                dismiss_message: Some(TestMessage::Dismiss),
                background_update: BackgroundUpdateDecision::Capture
            }
        );
    }

    #[test]
    fn pass_through_dismissible_outside_left_click_dismisses_and_updates_background() {
        let dismissal = dismissal(DismissedClickFlow::Continue);

        let decision = decide_floating_input(
            BackgroundInputPolicy::Interactive,
            Some(&dismissal),
            FloatingPointerTarget::Background,
            true,
            true,
        );

        assert_eq!(
            decision,
            FloatingInputDecision {
                dismiss_message: Some(TestMessage::Dismiss),
                background_update: BackgroundUpdateDecision::Update
            }
        );
    }
}
