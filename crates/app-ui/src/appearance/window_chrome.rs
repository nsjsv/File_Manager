use iced::widget::{button, container};
use iced::{Background, Border, Color, Theme};

use super::{
    base_text_color, button_hover_surface_color, button_pressed_surface_color, is_dark_theme,
    subtle_border_color,
};

pub(crate) fn window_title_bar_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(18, 24, 34)
        } else {
            Color::from_rgb8(250, 252, 255)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            ..Border::default()
        },
        ..container::Style::default()
    }
}

pub(crate) fn window_control_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Some(button_hover_surface_color(theme)),
        button::Status::Pressed => Some(button_pressed_surface_color(theme)),
        button::Status::Active | button::Status::Disabled => None,
    };
    button::Style {
        background: background.map(Background::Color),
        text_color: base_text_color(theme),
        border: Border {
            radius: 4.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

pub(crate) fn window_close_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Some(Color::from_rgb8(196, 43, 28)),
        button::Status::Pressed => Some(Color::from_rgb8(157, 34, 23)),
        button::Status::Active | button::Status::Disabled => None,
    };
    button::Style {
        background: background.map(Background::Color),
        text_color: if background.is_some() {
            Color::WHITE
        } else {
            base_text_color(theme)
        },
        border: Border {
            radius: 4.0.into(),
            ..Border::default()
        },
        ..button::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_button_uses_danger_feedback_only_while_interacting() {
        for theme in [Theme::Light, Theme::Dark] {
            let active = window_close_button_style(&theme, button::Status::Active);
            let hovered = window_close_button_style(&theme, button::Status::Hovered);
            let pressed = window_close_button_style(&theme, button::Status::Pressed);

            assert!(active.background.is_none());
            assert!(hovered.background.is_some());
            assert!(pressed.background.is_some());
            assert_eq!(hovered.text_color, Color::WHITE);
        }
    }

    #[test]
    fn ordinary_control_has_no_idle_background() {
        for theme in [Theme::Light, Theme::Dark] {
            let active = window_control_button_style(&theme, button::Status::Active);
            let hovered = window_control_button_style(&theme, button::Status::Hovered);

            assert!(active.background.is_none());
            assert!(hovered.background.is_some());
        }
    }
}
