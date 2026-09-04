use iced::widget::{button, column, container, row, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length, Theme};

use crate::app::FileBrowser;
use crate::appearance::{context_menu_button_style, muted_text_color, subtle_border_color};
use crate::config;
use crate::matugen_theme::ui_colors;
use crate::model::Message;
use crate::typography::{localized_text, readable_text};

use super::settings_group::info_setting_row;

/// 每行标签数量：标签按内容收缩，设置面板宽度下 8 个一行不溢出。
const CHIPS_PER_ROW: usize = 8;

/// 空格预览分类型后缀规则编辑：每个预览类型一行，标签式增删 + 恢复默认。
pub(super) fn preview_extension_rows(browser: &FileBrowser) -> Element<'_, Message> {
    let mut rows = column![].spacing(3);
    for (kind_index, kind) in config::PreviewFileSizeKind::ALL.iter().enumerate() {
        rows = rows.push(preview_extension_row(browser, kind_index, *kind));
    }
    rows.into()
}

fn preview_extension_row(
    browser: &FileBrowser,
    kind_index: usize,
    kind: config::PreviewFileSizeKind,
) -> Element<'_, Message> {
    let label = match kind {
        config::PreviewFileSizeKind::Text => "Text Preview Extensions",
        config::PreviewFileSizeKind::Image => "Image Preview Extensions",
        config::PreviewFileSizeKind::Video => "Video Preview Extensions",
        config::PreviewFileSizeKind::Audio => "Audio Preview Extensions",
        config::PreviewFileSizeKind::Archive => "Archive Preview Extensions",
        config::PreviewFileSizeKind::Document => "Document Preview Extensions",
        config::PreviewFileSizeKind::Sqlite => "SQLite Preview Extensions",
    };
    let input = text_input("txt", &browser.preview_extension_inputs[kind_index])
        .on_input(move |value| Message::PreviewExtensionInputChanged(kind_index, value))
        .on_submit(Message::PreviewExtensionInputCommitted(kind_index))
        .padding([6, 8])
        .size(12)
        .width(Length::Fixed(110.0));
    let add = button(container(readable_text("Add").size(12)).padding([6, 10]))
        .on_press(Message::PreviewExtensionInputCommitted(kind_index))
        .style(context_menu_button_style());
    let reset = button(readable_text("↺").size(12))
        .padding([4, 7])
        .style(chip_action_button_style())
        .on_press(Message::PreviewExtensionReset(kind_index));

    let mut content = column![row![
        readable_text(label).size(12).width(Length::Fill),
        reset,
        input,
        add,
    ]
    .spacing(8)
    .align_y(Alignment::Center)]
    .spacing(4);

    let extensions = browser.user_config().preview_extension_rules.list(kind);
    if !extensions.is_empty() {
        // 标签按文字内容收缩，每行固定数量手动换行；Grid 会把单元
        // 拉伸成等宽大方格，不适合紧凑标签。
        let chip_flow =
            extensions
                .chunks(CHIPS_PER_ROW)
                .fold(column![].spacing(4), |rows, chunk| {
                    let chips = chunk.iter().fold(row![].spacing(4), |row, extension| {
                        row.push(extension_chip(kind_index, extension))
                    });
                    rows.push(chips)
                });
        content = content.push(chip_flow);
    }
    if let Some(error) = &browser.preview_extension_input_errors[kind_index] {
        content = content.push(localized_text(error).size(11).width(Length::Fill));
    }

    info_setting_row(content.into())
}

fn extension_chip(kind_index: usize, extension: &str) -> Element<'_, Message> {
    let remove = button(readable_text("×").size(11))
        .padding([2, 6])
        .style(chip_remove_button_style())
        .on_press(Message::PreviewExtensionRemoved(
            kind_index,
            extension.to_owned(),
        ));
    container(
        row![readable_text(extension).size(11), remove]
            .spacing(2)
            .align_y(Alignment::Center),
    )
    .padding([2, 4])
    .style(chip_container_style)
    .into()
}

fn chip_action_button_style() -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let colors = ui_colors(theme);
        button::Style {
            background: Some(Background::Color(
                if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    colors.surface_container_high
                } else {
                    colors.surface_container
                },
            )),
            text_color: muted_text_color(theme),
            border: Border {
                color: subtle_border_color(theme),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..button::Style::default()
        }
    }
}

fn chip_container_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        border: Border {
            color: subtle_border_color(theme),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..container::Style::default()
    }
}

fn chip_remove_button_style() -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let colors = ui_colors(theme);
        button::Style {
            background: Some(Background::Color(
                if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    colors.surface_container_high
                } else {
                    Color::TRANSPARENT
                },
            )),
            text_color: muted_text_color(theme),
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}
