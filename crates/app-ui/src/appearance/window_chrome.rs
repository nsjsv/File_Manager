use iced::widget::{button, container};
use iced::{Background, Border, Color, Theme};

use super::{
    base_text_color, button_hover_surface_color, button_pressed_surface_color, subtle_border_color,
};
use crate::matugen_theme::ui_colors;

pub(crate) fn window_top_bar_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(window_chrome_background(theme))),
        text_color: Some(base_text_color(theme)),
        ..container::Style::default()
    }
}

pub(crate) fn window_title_bar_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(window_chrome_background(theme))),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            ..Border::default()
        },
        ..container::Style::default()
    }
}

fn window_chrome_background(theme: &Theme) -> Color {
    ui_colors(theme).background
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
    let colors = ui_colors(theme);
    let (background, text_color) = match status {
        button::Status::Hovered => (Some(colors.error), colors.on_error),
        button::Status::Pressed => (Some(colors.error_container), colors.on_error_container),
        button::Status::Active | button::Status::Disabled => (None, colors.on_surface),
    };
    button::Style {
        background: background.map(Background::Color),
        text_color,
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
    fn title_and_integrated_top_bars_share_the_window_surface() {
        for theme in [Theme::Light, Theme::Dark] {
            let title_bar = window_title_bar_style(&theme);
            let top_bar = window_top_bar_style(&theme);

            assert_eq!(title_bar.background, top_bar.background);
            assert_eq!(title_bar.text_color, top_bar.text_color);
            assert_eq!(top_bar.border.width, 0.0);
            assert_eq!(title_bar.border.width, 1.0);
        }
    }

    #[test]
    fn close_button_uses_danger_feedback_only_while_interacting() {
        for theme in [Theme::Light, Theme::Dark] {
            let active = window_close_button_style(&theme, button::Status::Active);
            let hovered = window_close_button_style(&theme, button::Status::Hovered);
            let pressed = window_close_button_style(&theme, button::Status::Pressed);

            assert!(active.background.is_none());
            assert!(hovered.background.is_some());
            assert!(pressed.background.is_some());
            assert_eq!(hovered.text_color, ui_colors(&theme).on_error);
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
