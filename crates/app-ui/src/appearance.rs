use iced::widget::{button, scrollable, svg};
use iced::{Background, Border, Color, Shadow, Theme, Vector};

mod container {
    pub use iced::widget::container::*;
    pub type Appearance = Style;
}

use crate::file_entry_presentation::SelectionRunPosition;
use crate::model::ScrollbarVisibility;

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
    selected_row_style_for_run(SelectionRunPosition::Single)(theme)
}

pub(crate) fn selected_row_style_for_run(
    position: SelectionRunPosition,
) -> impl Fn(&Theme) -> container::Appearance + Clone {
    move |theme| selected_row_appearance(theme, position)
}

fn selected_row_appearance(theme: &Theme, position: SelectionRunPosition) -> container::Appearance {
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
            radius: selected_run_radius(position),
            ..Border::default()
        },
        ..container::Appearance::default()
    }
}

fn selected_run_radius(position: SelectionRunPosition) -> iced::border::Radius {
    match position {
        SelectionRunPosition::Single => iced::border::Radius::new(8.0),
        SelectionRunPosition::First => iced::border::Radius::default().top(8.0),
        SelectionRunPosition::Middle => iced::border::Radius::default(),
        SelectionRunPosition::Last => iced::border::Radius::default().bottom(8.0),
    }
}

pub(crate) fn open_child_row_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(32, 43, 59)
        } else {
            Color::from_rgb8(235, 240, 248)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: if is_dark_theme(theme) {
                Color::from_rgb8(72, 94, 127)
            } else {
                Color::from_rgb8(201, 212, 229)
            },
            width: 1.0,
            radius: 8.0.into(),
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

pub(crate) fn tab_split_overlay_style(theme: &Theme) -> container::Appearance {
    let accent = if is_dark_theme(theme) {
        Color::from_rgb8(125, 179, 255)
    } else {
        Color::from_rgb8(74, 137, 220)
    };
    container::Appearance {
        background: Some(Background::Color(Color { a: 0.18, ..accent })),
        border: Border {
            color: Color { a: 0.74, ..accent },
            width: 1.0,
            radius: 0.0.into(),
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

pub(crate) fn navigation_icon_button_style() -> fn(&Theme, button::Status) -> button::Style {
    transparent_button_style
}

pub(crate) fn context_menu_button_style() -> fn(&Theme, button::Status) -> button::Style {
    transparent_button_style
}

pub(crate) fn auto_hide_scrollbar_style(
    visibility: ScrollbarVisibility,
) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style + Clone {
    move |theme, status| mac_scrollbar_style(theme, status, visibility)
}

pub(crate) fn auto_hide_vertical_scrollbar_direction(
    visibility: ScrollbarVisibility,
    width: f32,
) -> scrollable::Direction {
    scrollable::Direction::Vertical(auto_hide_scrollbar_properties(visibility, width))
}

pub(crate) fn auto_hide_horizontal_scrollbar_direction(
    visibility: ScrollbarVisibility,
    width: f32,
) -> scrollable::Direction {
    scrollable::Direction::Horizontal(auto_hide_scrollbar_properties(visibility, width))
}

fn auto_hide_scrollbar_properties(
    visibility: ScrollbarVisibility,
    width: f32,
) -> scrollable::Scrollbar {
    let width = if visibility.opacity() <= f32::EPSILON {
        0.0
    } else {
        width
    };

    scrollable::Scrollbar::new()
        .width(width)
        .scroller_width(width)
}

pub(crate) fn path_suggestions_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(25, 32, 44)
        } else {
            Color::from_rgb8(250, 251, 253)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 8.0.into(),
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
            Color::from_rgb8(24, 31, 43)
        } else {
            Color::from_rgb8(245, 247, 250)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 14.0.into(),
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn preview_window_panel_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(24, 31, 43)
        } else {
            Color::from_rgb8(245, 247, 250)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 14.0.into(),
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn error_notification_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(55, 31, 34)
        } else {
            Color::from_rgb8(255, 247, 247)
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
        ..container::Appearance::default()
    }
}

pub(crate) fn column_browser_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        text_color: Some(base_text_color(theme)),
        ..container::Appearance::default()
    }
}

pub(crate) fn column_panel_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        text_color: Some(base_text_color(theme)),
        ..container::Appearance::default()
    }
}

pub(crate) fn list_panel_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        text_color: Some(base_text_color(theme)),
        ..container::Appearance::default()
    }
}

pub(crate) fn list_header_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgba8(27, 35, 48, 0.88)
        } else {
            Color::from_rgba8(239, 243, 249, 0.92)
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

pub(crate) fn list_row_style(
    _depth: usize,
    row_index: usize,
) -> impl Fn(&Theme) -> container::Appearance + Clone {
    move |theme| {
        let is_alternate_row = row_index % 2 == 1;
        let background = if is_dark_theme(theme) {
            if is_alternate_row {
                Color::from_rgb8(25, 33, 45)
            } else {
                Color::from_rgb8(18, 24, 34)
            }
        } else if is_alternate_row {
            Color::from_rgb8(242, 246, 252)
        } else {
            Color::from_rgb8(250, 252, 255)
        };
        container::Appearance {
            background: Some(Background::Color(background)),
            text_color: Some(base_text_color(theme)),
            border: Border {
                radius: if is_alternate_row { 7.0 } else { 0.0 }.into(),
                ..Border::default()
            },
            ..container::Appearance::default()
        }
    }
}

pub(crate) fn column_resize_divider_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(subtle_border_color(theme))),
        ..container::Appearance::default()
    }
}

pub(crate) fn sidebar_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgba8(27, 34, 46, 0.92)
        } else {
            Color::from_rgba8(255, 255, 255, 0.90)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: if is_dark_theme(theme) {
                Color::from_rgba8(98, 112, 134, 0.38)
            } else {
                Color::from_rgba8(255, 255, 255, 0.72)
            },
            width: 1.0,
            radius: 18.0.into(),
        },
        shadow: Shadow {
            color: if is_dark_theme(theme) {
                Color::from_rgba8(0, 0, 0, 0.34)
            } else {
                Color::from_rgba8(36, 48, 70, 0.16)
            },
            offset: Vector::new(0.0, 10.0),
            blur_radius: 22.0,
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

pub(crate) fn tab_strip_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgba8(27, 35, 48, 0.78)
        } else {
            Color::from_rgba8(239, 243, 249, 0.84)
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

pub(crate) fn tab_item_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgba8(40, 49, 63, 0.72)
        } else {
            Color::from_rgba8(250, 252, 255, 0.74)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn selected_tab_item_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(54, 78, 116)
        } else {
            Color::from_rgb8(255, 255, 255)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: if is_dark_theme(theme) {
                Color::from_rgb8(90, 128, 184)
            } else {
                Color::from_rgb8(201, 213, 232)
            },
            width: 1.0,
            radius: 12.0.into(),
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

pub(crate) fn sidebar_bookmark_drop_slot_style(theme: &Theme) -> container::Appearance {
    let accent = if is_dark_theme(theme) {
        Color::from_rgb8(125, 179, 255)
    } else {
        Color::from_rgb8(74, 137, 220)
    };
    container::Appearance {
        background: Some(Background::Color(accent)),
        border: Border {
            radius: 1.0.into(),
            ..Border::default()
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn context_menu_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(24, 31, 43)
        } else {
            Color::from_rgb8(250, 251, 253)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Appearance::default()
    }
}

pub(crate) fn drag_preview_style(theme: &Theme) -> container::Appearance {
    container::Appearance {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(31, 38, 50)
        } else {
            Color::from_rgb8(255, 255, 255)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 10.0.into(),
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
        ..container::Appearance::default()
    }
}

pub(crate) fn icon_svg_style() -> fn(&Theme, svg::Status) -> svg::Style {
    icon_svg_style_for_status
}

pub(crate) fn selected_icon_svg_style() -> fn(&Theme, svg::Status) -> svg::Style {
    selected_icon_svg_style_for_status
}

pub(crate) fn muted_icon_svg_style() -> fn(&Theme, svg::Status) -> svg::Style {
    muted_icon_svg_style_for_status
}

pub(crate) fn warning_icon_svg_style() -> fn(&Theme, svg::Status) -> svg::Style {
    warning_icon_svg_style_for_status
}

fn icon_svg_style_for_status(theme: &Theme, _status: svg::Status) -> svg::Style {
    svg::Style {
        color: Some(if is_dark_theme(theme) {
            Color::from_rgb8(191, 203, 220)
        } else {
            Color::from_rgb8(68, 77, 90)
        }),
    }
}

fn selected_icon_svg_style_for_status(theme: &Theme, _status: svg::Status) -> svg::Style {
    svg::Style {
        color: Some(if is_dark_theme(theme) {
            Color::from_rgb8(236, 244, 255)
        } else {
            Color::from_rgb8(24, 42, 72)
        }),
    }
}

fn muted_icon_svg_style_for_status(theme: &Theme, _status: svg::Status) -> svg::Style {
    svg::Style {
        color: Some(if is_dark_theme(theme) {
            Color::from_rgb8(137, 146, 159)
        } else {
            Color::from_rgb8(119, 127, 139)
        }),
    }
}

fn warning_icon_svg_style_for_status(_theme: &Theme, _status: svg::Status) -> svg::Style {
    svg::Style {
        color: Some(Color::from_rgb8(180, 83, 9)),
    }
}

fn transparent_button_style(theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        text_color: base_text_color(theme),
        ..button::Style::default()
    }
}

fn mac_scrollbar_style(
    theme: &Theme,
    status: scrollable::Status,
    visibility: ScrollbarVisibility,
) -> scrollable::Style {
    let mut opacity = visibility.opacity();

    match status {
        scrollable::Status::Hovered {
            is_horizontal_scrollbar_hovered,
            is_vertical_scrollbar_hovered,
            ..
        } if opacity > 0.0
            && (is_horizontal_scrollbar_hovered || is_vertical_scrollbar_hovered) =>
        {
            opacity = (opacity + 0.18).min(1.0);
        }
        scrollable::Status::Dragged {
            is_horizontal_scrollbar_dragged,
            is_vertical_scrollbar_dragged,
            ..
        } if opacity > 0.0
            && (is_horizontal_scrollbar_dragged || is_vertical_scrollbar_dragged) =>
        {
            opacity = opacity.max(0.86);
        }
        _ => {}
    }

    let mut style = scrollable::default(theme, status);
    let scroller_background = mac_scrollbar_scroller_color(theme, opacity).into();
    let rail_border = Border {
        radius: 999.0.into(),
        ..Border::default()
    };
    let scroller_border = Border {
        radius: 999.0.into(),
        ..Border::default()
    };

    style.vertical_rail.background = None;
    style.vertical_rail.border = rail_border;
    style.vertical_rail.scroller.background = scroller_background;
    style.vertical_rail.scroller.border = scroller_border;
    style.horizontal_rail.background = None;
    style.horizontal_rail.border = rail_border;
    style.horizontal_rail.scroller.background = scroller_background;
    style.horizontal_rail.scroller.border = scroller_border;
    style.gap = None;
    style
}

fn mac_scrollbar_scroller_color(theme: &Theme, opacity: f32) -> Color {
    let opacity = opacity.clamp(0.0, 1.0);
    if is_dark_theme(theme) {
        Color::from_rgba8(255, 255, 255, 0.42 * opacity)
    } else {
        Color::from_rgba8(20, 24, 31, 0.32 * opacity)
    }
}

pub(crate) fn base_text_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(226, 233, 242)
    } else {
        Color::from_rgb8(36, 43, 54)
    }
}

pub(crate) fn muted_text_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(137, 146, 159)
    } else {
        Color::from_rgb8(119, 127, 139)
    }
}

pub(crate) fn subtle_border_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(54, 65, 82)
    } else {
        Color::from_rgb8(218, 225, 235)
    }
}

pub(crate) fn is_dark_theme(theme: &Theme) -> bool {
    let background = theme.palette().background;
    background.r * 0.299 + background.g * 0.587 + background.b * 0.114 < 0.5
}
