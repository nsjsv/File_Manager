use iced::widget::{container, svg};
use iced::{theme, Background, Border, Color, Shadow, Theme, Vector};

pub(crate) fn app_content_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(18, 24, 34)
        } else {
            Color::from_rgb8(250, 252, 255)
        })),
        text_color: Some(base_text_color(theme)),
        ..container::Appearance::default()
    }
}

pub(crate) fn selected_row_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(54, 78, 116)
        } else {
            Color::from_rgb8(218, 232, 255)
        })),
        text_color: Some(if is_dark_theme(theme) {
            Color::from_rgb8(236, 244, 255)
        } else {
            Color::from_rgb8(24, 42, 72)
        }),
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn dragged_row_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(39, 44, 52)
        } else {
            Color::from_rgb8(229, 233, 238)
        })),
        text_color: Some(if is_dark_theme(theme) {
            Color::from_rgb8(150, 159, 172)
        } else {
            Color::from_rgb8(119, 127, 139)
        }),
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn selection_marquee_style(theme: &Theme) -> container::Appearance {
    let accent = if is_dark_theme(theme) {
        Color::from_rgb8(125, 179, 255)
    } else {
        Color::from_rgb8(74, 137, 220)
    };
    container::Appearance {
        background: Some(Background::Color(Color { a: 0.16, ..accent })),
        border: Border {
            color: accent,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn hovered_row_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(35, 47, 65)
        } else {
            Color::from_rgb8(239, 245, 255)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn navigation_icon_button_style() -> theme::Button {
    theme::Button::custom(TransparentButtonStyle)
}

pub(crate) fn context_menu_button_style() -> theme::Button {
    theme::Button::custom(TransparentButtonStyle)
}

pub(crate) fn path_suggestions_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgba8(25, 32, 44, 0.98)
        } else {
            Color::from_rgba8(250, 251, 253, 0.98)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(0, 0, 0, if is_dark_theme(theme) { 0.35 } else { 0.16 }),
            offset: Vector::new(0.0, 10.0),
            blur_radius: 22.0,
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn path_suggestion_item_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(31, 40, 54)
        } else {
            Color::from_rgb8(244, 247, 252)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn selected_path_suggestion_item_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(52, 75, 112)
        } else {
            Color::from_rgb8(219, 233, 255)
        })),
        text_color: Some(if is_dark_theme(theme) {
            Color::from_rgb8(236, 244, 255)
        } else {
            Color::from_rgb8(24, 42, 72)
        }),
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn preview_panel_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgba8(24, 31, 43, 0.97)
        } else {
            Color::from_rgba8(245, 247, 250, 0.97)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 14.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.22),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 28.0,
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn error_notification_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgba8(55, 31, 34, 0.97)
        } else {
            Color::from_rgba8(255, 247, 247, 0.98)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: if is_dark_theme(theme) {
                Color::from_rgb8(127, 55, 63)
            } else {
                Color::from_rgb8(252, 165, 165)
            },
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.2),
            offset: Vector::new(0.0, 10.0),
            blur_radius: 24.0,
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn column_browser_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(18, 24, 34)
        } else {
            Color::from_rgb8(250, 252, 255)
        })),
        text_color: Some(base_text_color(theme)),
        ..container::Appearance::default()
    }
}

pub(crate) fn column_panel_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgba8(21, 28, 39, 0.92)
        } else {
            Color::from_rgba8(255, 255, 255, 0.78)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            ..Border::default()
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn column_resize_divider_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgba8(54, 65, 82, 0.52)
        } else {
            Color::from_rgba8(218, 225, 235, 0.72)
        })),
        ..container::Appearance::default()
    }
}

pub(crate) fn sidebar_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(22, 29, 40)
        } else {
            Color::from_rgb8(238, 241, 246)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            ..Border::default()
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn selected_sidebar_item_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(49, 70, 104)
        } else {
            Color::from_rgb8(224, 235, 255)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn hovered_sidebar_item_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(32, 41, 55)
        } else {
            Color::from_rgb8(245, 248, 253)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            radius: 8.0.into(),
            ..Border::default()
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn context_menu_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgba8(24, 31, 43, 0.98)
        } else {
            Color::from_rgba8(250, 251, 253, 0.98)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.18),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 18.0,
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn drag_preview_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgba8(31, 38, 50, 0.94)
        } else {
            Color::from_rgba8(255, 255, 255, 0.96)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 10.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(0, 0, 0, if is_dark_theme(theme) { 0.35 } else { 0.18 }),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 18.0,
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn switch_track_on_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(82, 126, 190)
        } else {
            Color::from_rgb8(74, 137, 220)
        })),
        border: Border {
            radius: 11.0.into(),
            ..Border::default()
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn switch_track_off_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(54, 65, 82)
        } else {
            Color::from_rgb8(210, 218, 230)
        })),
        border: Border {
            radius: 11.0.into(),
            ..Border::default()
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn switch_thumb_style(_theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(Color::WHITE)),
        border: Border {
            radius: 7.0.into(),
            ..Border::default()
        },
        shadow: Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.22),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 3.0,
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn icon_svg_style() -> theme::Svg {
    theme::Svg::custom_fn(icon_svg_appearance)
}

pub(crate) fn selected_icon_svg_style() -> theme::Svg {
    theme::Svg::custom_fn(selected_icon_svg_appearance)
}

pub(crate) fn muted_icon_svg_style() -> theme::Svg {
    theme::Svg::custom_fn(muted_icon_svg_appearance)
}

pub(crate) fn warning_icon_svg_style() -> theme::Svg {
    theme::Svg::custom_fn(warning_icon_svg_appearance)
}

fn icon_svg_appearance(theme: &Theme) -> svg::Appearance {
    svg::Appearance {
        color: Some(if is_dark_theme(theme) {
            Color::from_rgb8(191, 203, 220)
        } else {
            Color::from_rgb8(68, 77, 90)
        }),
    }
}

fn selected_icon_svg_appearance(theme: &Theme) -> svg::Appearance {
    svg::Appearance {
        color: Some(if is_dark_theme(theme) {
            Color::from_rgb8(236, 244, 255)
        } else {
            Color::from_rgb8(24, 42, 72)
        }),
    }
}

fn muted_icon_svg_appearance(theme: &Theme) -> svg::Appearance {
    svg::Appearance {
        color: Some(if is_dark_theme(theme) {
            Color::from_rgb8(137, 146, 159)
        } else {
            Color::from_rgb8(119, 127, 139)
        }),
    }
}

fn warning_icon_svg_appearance(_theme: &Theme) -> svg::Appearance {
    svg::Appearance {
        color: Some(Color::from_rgb8(180, 83, 9)),
    }
}

struct TransparentButtonStyle;

impl iced::widget::button::StyleSheet for TransparentButtonStyle {
    type Style = Theme;

    fn active(&self, style: &Self::Style) -> iced::widget::button::Appearance {
        transparent_button_appearance(style)
    }

    fn hovered(&self, style: &Self::Style) -> iced::widget::button::Appearance {
        transparent_button_appearance(style)
    }

    fn pressed(&self, style: &Self::Style) -> iced::widget::button::Appearance {
        transparent_button_appearance(style)
    }
}

fn transparent_button_appearance(theme: &Theme) -> iced::widget::button::Appearance {
    iced::widget::button::Appearance {
        text_color: base_text_color(theme),
        ..iced::widget::button::Appearance::default()
    }
}

fn base_text_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(226, 233, 242)
    } else {
        Color::from_rgb8(36, 43, 54)
    }
}

fn subtle_border_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(54, 65, 82)
    } else {
        Color::from_rgb8(218, 225, 235)
    }
}

fn is_dark_theme(theme: &Theme) -> bool {
    let background = theme.palette().background;
    background.r * 0.299 + background.g * 0.587 + background.b * 0.114 < 0.5
}
