use iced::widget::{container, Space};
use iced::{Element, Length};

use crate::appearance::selection_marquee_style;
use crate::floating_surface::{FloatingContent, FloatingPlacement};
use crate::model::{Message, SelectionMarquee};

pub(crate) fn selection_marquee_overlay(
    marquee: &SelectionMarquee,
) -> FloatingContent<'static, Message> {
    let top_left = marquee.top_left();
    let element: Element<'static, Message> = container(Space::new(
        Length::Fixed(marquee.width()),
        Length::Fixed(marquee.height()),
    ))
    .style(selection_marquee_style)
    .into();

    FloatingContent {
        element,
        placement: FloatingPlacement::Free(top_left),
    }
}
