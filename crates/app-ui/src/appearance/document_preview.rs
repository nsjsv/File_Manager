use iced::widget::container;
use iced::{Background, Border, Color, Theme};

use super::{is_dark_theme, subtle_border_color};

pub(crate) fn document_page_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(38, 43, 51)
        } else {
            Color::from_rgb8(250, 251, 253)
        })),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 3.0.into(),
        },
        ..container::Style::default()
    }
}
