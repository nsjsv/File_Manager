use std::any::Any;
use std::path::PathBuf;

use iced::advanced::widget::operation::Outcome;
use iced::advanced::{layout, overlay, renderer, widget, Clipboard, Layout, Shell, Widget};
use iced::{mouse, Element, Event, Length, Rectangle, Size, Task, Vector};

use crate::model::{BrowserPaneId, ColumnEntryBounds, Message};

pub(crate) fn track_column_entry_bounds<'a>(
    content: impl Into<Element<'a, Message>>,
    pane_id: BrowserPaneId,
    path: PathBuf,
) -> Element<'a, Message> {
    Element::new(ColumnEntryBoundsTracker {
        content: content.into(),
        pane_id,
        path,
    })
}

pub(crate) fn column_entry_bounds_command() -> Task<Message> {
    widget::operate(ColumnEntryBoundsOperation::default())
}

#[derive(Debug, Clone)]
struct ColumnEntryBoundsMarker {
    pane_id: BrowserPaneId,
    path: PathBuf,
}

#[derive(Debug, Default)]
struct ColumnEntryBoundsOperation {
    bounds: Vec<ColumnEntryBounds>,
}

impl widget::Operation<Message> for ColumnEntryBoundsOperation {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation<Message>)) {
        operate(self);
    }

    fn custom(&mut self, _id: Option<&widget::Id>, bounds: Rectangle, state: &mut dyn Any) {
        let Some(marker) = state.downcast_ref::<ColumnEntryBoundsMarker>() else {
            return;
        };
        self.bounds.push(ColumnEntryBounds {
            pane_id: marker.pane_id,
            path: marker.path.clone(),
            bounds,
        });
    }

    fn finish(&self) -> Outcome<Message> {
        Outcome::Some(Message::ColumnEntryBoundsMeasured(self.bounds.clone()))
    }
}

struct ColumnEntryBoundsTracker<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    content: Element<'a, Message, Theme, Renderer>,
    pane_id: BrowserPaneId,
    path: PathBuf,
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for ColumnEntryBoundsTracker<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn children(&self) -> Vec<widget::Tree> {
        vec![widget::Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
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
        let mut marker = ColumnEntryBoundsMarker {
            pane_id: self.pane_id,
            path: self.path.clone(),
        };
        operation.custom(None, layout.bounds(), &mut marker);
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
            cursor,
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
            cursor,
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
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
