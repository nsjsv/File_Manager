use iced::{Color, Theme};

use crate::appearance::is_dark_theme;

pub(super) fn viewer_background_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(20, 27, 38)
    } else {
        Color::from_rgb8(250, 251, 253)
    }
}

pub(super) fn divider_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(62, 76, 101)
    } else {
        Color::from_rgb8(211, 219, 232)
    }
}

pub(super) fn text_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(236, 244, 255)
    } else {
        Color::from_rgb8(24, 42, 72)
    }
}

pub(super) fn placeholder_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(137, 146, 159)
    } else {
        Color::from_rgb8(119, 127, 139)
    }
}

pub(super) fn selection_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgba8(85, 135, 205, 0.42)
    } else {
        Color::from_rgba8(150, 190, 255, 0.62)
    }
}
