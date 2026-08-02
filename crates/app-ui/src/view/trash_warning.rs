use iced::widget::{button, column, container, row};
use iced::{Alignment, Background, Border, Color, Element, Length, Theme};

use crate::app::FileBrowser;
use crate::appearance::{base_text_color, is_dark_theme, transparent_button_style};
use crate::formatting::format_middle_ellipsized_text;
use crate::icons::IconSymbol;
use crate::localization;
use crate::model::Message;
use crate::typography::{localized_text, readable_text};

use super::{themed_icon, IconTone, MENU_ICON_SIZE};

const MAX_VISIBLE_WARNING_DETAILS: usize = 50;
const WARNING_DETAIL_MAX_CHARS: usize = 140;

fn trash_warning_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(if is_dark_theme(theme) {
            Color::from_rgb8(53, 43, 24)
        } else {
            Color::from_rgb8(255, 250, 235)
        })),
        text_color: Some(base_text_color(theme)),
        border: Border {
            color: if is_dark_theme(theme) {
                Color::from_rgb8(133, 98, 43)
            } else {
                Color::from_rgb8(217, 167, 74)
            },
            width: 1.0,
            radius: 6.0.into(),
        },
        ..container::Style::default()
    }
}

pub(super) fn trash_warning_panel(browser: &FileBrowser) -> Option<Element<'_, Message>> {
    let warnings = browser
        .trash_refresh
        .snapshot()
        .map(|snapshot| snapshot.skipped.as_slice())
        .unwrap_or_default();
    let refresh_error = browser.trash_refresh.last_error();
    if warnings.is_empty() && refresh_error.is_none() {
        return None;
    }

    let expanded = browser.trash_refresh.warning_details_expanded();
    let summary_text = refresh_error.map_or_else(
        || localization::trash_warning_summary(warnings.len()),
        localization::trash_refresh_failed,
    );
    let mut summary = row![
        themed_icon(IconSymbol::TriangleAlert, IconTone::Warning, MENU_ICON_SIZE),
        readable_text(summary_text).size(12).width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .width(Length::Fill);
    if !warnings.is_empty() {
        let action_label = if expanded {
            "Hide warning details"
        } else {
            "Show warning details"
        };
        summary = summary.push(
            button(localized_text(action_label).size(11))
                .padding([4, 6])
                .style(transparent_button_style())
                .on_press(Message::TrashWarningsToggled),
        );
    }

    let mut content = column![summary].spacing(5).width(Length::Fill);
    if expanded {
        for warning in warnings.iter().take(MAX_VISIBLE_WARNING_DETAILS) {
            let detail = format!("{}: {}", warning.path.display(), warning.message);
            content = content.push(
                readable_text(format_middle_ellipsized_text(
                    &detail,
                    WARNING_DETAIL_MAX_CHARS,
                ))
                .size(11)
                .width(Length::Fill),
            );
        }
        if warnings.len() > MAX_VISIBLE_WARNING_DETAILS {
            content = content.push(
                readable_text(localization::trash_additional_warning_count(
                    warnings.len() - MAX_VISIBLE_WARNING_DETAILS,
                ))
                .size(11),
            );
        }
    }

    Some(
        container(container(content).padding(10).style(trash_warning_style))
            .padding([8, 18])
            .width(Length::Fill)
            .into(),
    )
}
