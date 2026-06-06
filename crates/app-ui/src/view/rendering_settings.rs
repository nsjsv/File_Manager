use iced::widget::{button, column, container, row, Button, Space};
use iced::{Alignment, Element, Length};

use crate::appearance::{context_menu_button_style, context_menu_style};
use crate::config::RenderingBackendPreference;
use crate::icons::IconSymbol;
use crate::model::Message;
use crate::typography::readable_text;

use super::toggle_switch::switch_control;
use super::{themed_icon, IconTone, MENU_ICON_SIZE};

const RENDERER_RESTART_NOTICE_WIDTH: f32 = 420.0;

pub(super) fn gpu_rendering_button(
    preference: RenderingBackendPreference,
) -> Button<'static, Message> {
    let label = row![
        readable_text("GPU Rendering").size(12).width(Length::Fill),
        readable_text(rendering_status(preference)).size(12),
        switch_control(matches!(preference, RenderingBackendPreference::Gpu)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    button(container(label).padding([5, 8]).width(Length::Fill))
        .on_press(Message::RenderingBackendPreferenceSelected(
            next_rendering_backend_preference(preference),
        ))
        .width(Length::Fill)
        .style(context_menu_button_style())
}

pub(super) fn renderer_restart_notice_panel() -> Element<'static, Message> {
    let title = row![
        themed_icon(IconSymbol::TriangleAlert, IconTone::Warning, MENU_ICON_SIZE),
        readable_text("Restart Required")
            .size(16)
            .width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let actions = row![
        Space::new().width(Length::Fill),
        button(readable_text("OK").size(12))
            .on_press(Message::RendererRestartNoticeDismissed)
            .padding([6, 10])
            .style(context_menu_button_style()),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    container(
        column![
            title,
            readable_text(
                "Rendering backend changes will take effect after restarting File Manager."
            )
            .size(13),
            actions,
        ]
        .spacing(12)
        .width(Length::Fill),
    )
    .padding(14)
    .width(Length::Fixed(RENDERER_RESTART_NOTICE_WIDTH))
    .style(context_menu_style)
    .into()
}

fn rendering_status(preference: RenderingBackendPreference) -> &'static str {
    match preference {
        RenderingBackendPreference::Software => "Off",
        RenderingBackendPreference::Gpu => "On",
    }
}

fn next_rendering_backend_preference(
    preference: RenderingBackendPreference,
) -> RenderingBackendPreference {
    match preference {
        RenderingBackendPreference::Software => RenderingBackendPreference::Gpu,
        RenderingBackendPreference::Gpu => RenderingBackendPreference::Software,
    }
}
