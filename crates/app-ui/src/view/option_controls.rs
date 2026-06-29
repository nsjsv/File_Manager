use iced::widget::{button, container, row, Button, Column};
use iced::{Alignment, Background, Border, Color, Element, Length, Shadow, Theme, Vector};

use crate::appearance::{base_text_color, is_dark_theme, muted_text_color, subtle_border_color};
use crate::model::Message;
use crate::typography::readable_text;

const SEGMENTED_CHOICE_HEIGHT: f32 = 30.0;
const ACTION_CHOICE_HEIGHT: f32 = 58.0;

pub(super) struct SegmentedChoice {
    pub(super) label: &'static str,
    pub(super) selected: bool,
    pub(super) message: Message,
}

pub(super) fn segmented_choice_row(choices: Vec<SegmentedChoice>) -> Element<'static, Message> {
    let mut options = row![].spacing(0).align_y(Alignment::Center);
    for choice in choices {
        options = options.push(segmented_choice_button(choice));
    }

    container(options)
        .padding(2)
        .width(Length::Fill)
        .style(segmented_choice_group_style)
        .into()
}

pub(super) fn selectable_choice_row(
    title: &'static str,
    description: &'static str,
    selected: bool,
    message: Message,
) -> Element<'static, Message> {
    let labels = Column::new()
        .spacing(2)
        .push(readable_text(title).size(12).width(Length::Fill))
        .push(readable_text(description).size(11).width(Length::Fill));
    let button = button(labels)
        .padding([7, 10])
        .width(Length::Fill)
        .height(Length::Fixed(ACTION_CHOICE_HEIGHT))
        .style(selectable_choice_button_style(selected));

    if selected {
        button.into()
    } else {
        button.on_press(message).into()
    }
}

pub(super) fn action_choice_row(
    title: &'static str,
    description: &'static str,
    message: Message,
) -> Element<'static, Message> {
    let labels = Column::new()
        .spacing(2)
        .push(readable_text(title).size(12).width(Length::Fill))
        .push(readable_text(description).size(11).width(Length::Fill));

    button(labels)
        .on_press(message)
        .padding([7, 10])
        .width(Length::Fill)
        .height(Length::Fixed(ACTION_CHOICE_HEIGHT))
        .style(selectable_choice_button_style(false))
        .into()
}

pub(super) fn primary_action_button(
    label: &'static str,
    message: Message,
) -> Button<'static, Message> {
    button(readable_text(label).size(12))
        .on_press(message)
        .padding([6, 12])
        .style(primary_action_button_style())
}

pub(super) fn inactive_primary_action_button(label: &'static str) -> Button<'static, Message> {
    button(readable_text(label).size(12))
        .padding([6, 12])
        .style(primary_action_button_style())
}

pub(super) fn secondary_action_button(
    label: &'static str,
    message: Message,
) -> Button<'static, Message> {
    button(readable_text(label).size(12))
        .on_press(message)
        .padding([6, 12])
        .style(secondary_action_button_style())
}

fn segmented_choice_button(choice: SegmentedChoice) -> Button<'static, Message> {
    let label = container(readable_text(choice.label).size(12))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    button(label)
        .on_press(choice.message)
        .width(Length::FillPortion(1))
        .height(Length::Fixed(SEGMENTED_CHOICE_HEIGHT))
        .padding([4, 8])
        .style(segmented_choice_button_style(choice.selected))
}

fn segmented_choice_group_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(18, 24, 34)
        } else {
            Color::from_rgb8(239, 243, 249)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}

fn segmented_choice_button_style(
    selected: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + Clone {
    move |theme, status| {
        let background = segmented_choice_background(theme, status, selected);
        let border_color = if selected {
            accent_border_color(theme)
        } else {
            subtle_border_color(theme)
        };

        button::Style {
            background: Some(Background::Color(background)),
            text_color: if selected {
                selected_text_color(theme)
            } else {
                base_text_color(theme)
            },
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..button::Style::default()
        }
    }
}

fn selectable_choice_button_style(
    selected: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + Clone {
    move |theme, status| {
        let background = if selected {
            match status {
                button::Status::Hovered => selected_hover_background_color(theme),
                button::Status::Pressed => selected_pressed_background_color(theme),
                _ => selected_background_color(theme),
            }
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => hover_background_color(theme),
                button::Status::Active | button::Status::Disabled => panel_background_color(theme),
            }
        };

        button::Style {
            background: Some(Background::Color(background)),
            text_color: if selected {
                selected_text_color(theme)
            } else {
                base_text_color(theme)
            },
            border: Border {
                color: if selected {
                    accent_border_color(theme)
                } else {
                    subtle_border_color(theme)
                },
                width: 1.0,
                radius: 8.0.into(),
            },
            ..button::Style::default()
        }
    }
}

fn primary_action_button_style() -> fn(&Theme, button::Status) -> button::Style {
    primary_action_button_appearance
}

fn primary_action_button_appearance(theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => accent_hover_color(theme),
        button::Status::Pressed => accent_pressed_color(theme),
        button::Status::Disabled => disabled_action_background(theme),
        button::Status::Active => accent_color(theme),
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: if matches!(status, button::Status::Disabled) {
            muted_text_color(theme)
        } else {
            Color::WHITE
        },
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 7.0.into(),
        },
        shadow: action_button_shadow(theme, status),
        ..button::Style::default()
    }
}

pub(super) fn secondary_action_button_style() -> fn(&Theme, button::Status) -> button::Style {
    secondary_action_button_appearance
}

fn secondary_action_button_appearance(theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => hover_background_color(theme),
        button::Status::Active | button::Status::Disabled => panel_background_color(theme),
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: if matches!(status, button::Status::Disabled) {
            muted_text_color(theme)
        } else {
            base_text_color(theme)
        },
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 7.0.into(),
        },
        ..button::Style::default()
    }
}

fn segmented_choice_background(theme: &Theme, status: button::Status, selected: bool) -> Color {
    match (selected, status) {
        (true, button::Status::Hovered) => selected_hover_background_color(theme),
        (true, button::Status::Pressed) => selected_pressed_background_color(theme),
        (true, _) => selected_background_color(theme),
        (false, button::Status::Hovered) | (false, button::Status::Pressed) => {
            hover_background_color(theme)
        }
        (false, _) => panel_background_color(theme),
    }
}

fn accent_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(82, 126, 190)
    } else {
        Color::from_rgb8(74, 137, 220)
    }
}

fn accent_hover_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(96, 144, 210)
    } else {
        Color::from_rgb8(59, 122, 203)
    }
}

fn accent_pressed_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(70, 109, 169)
    } else {
        Color::from_rgb8(48, 104, 177)
    }
}

fn accent_border_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(106, 153, 221)
    } else {
        Color::from_rgb8(134, 174, 232)
    }
}

fn selected_background_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(49, 70, 104)
    } else {
        Color::from_rgb8(224, 235, 255)
    }
}

fn selected_hover_background_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(56, 80, 118)
    } else {
        Color::from_rgb8(213, 229, 255)
    }
}

fn selected_pressed_background_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(45, 64, 96)
    } else {
        Color::from_rgb8(202, 222, 255)
    }
}

fn selected_text_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(236, 244, 255)
    } else {
        Color::from_rgb8(24, 42, 72)
    }
}

fn panel_background_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(31, 40, 54)
    } else {
        Color::from_rgb8(244, 247, 252)
    }
}

fn hover_background_color(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(35, 47, 65)
    } else {
        Color::from_rgb8(239, 245, 255)
    }
}

fn disabled_action_background(theme: &Theme) -> Color {
    if is_dark_theme(theme) {
        Color::from_rgb8(54, 65, 82)
    } else {
        Color::from_rgb8(210, 218, 230)
    }
}

fn action_button_shadow(theme: &Theme, status: button::Status) -> Shadow {
    if matches!(status, button::Status::Disabled) {
        Shadow::default()
    } else {
        Shadow {
            color: if is_dark_theme(theme) {
                Color::from_rgba8(0, 0, 0, 0.18)
            } else {
                Color::from_rgba8(36, 48, 70, 0.12)
            },
            offset: Vector::new(0.0, 1.0),
            blur_radius: 3.0,
        }
    }
}
