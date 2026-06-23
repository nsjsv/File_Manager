use iced::widget::{button, column, container, row, scrollable, Column, Space};
use iced::{Alignment, Element, Length};

use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use crate::app::FileBrowser;
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, context_menu_button_style,
};
use crate::model::{Message, ScrollbarRegion};
use crate::shortcuts::{ShortcutBinding, ShortcutCaptureState};
use crate::typography::readable_text;

const SHORTCUT_LIST_HEIGHT: f32 = 300.0;
const SHORTCUT_BINDING_BUTTON_WIDTH: f32 = 142.0;

pub(super) fn shortcut_settings_section(browser: &FileBrowser) -> Element<'_, Message> {
    let mut shortcut_rows = Column::new().spacing(4);
    for binding in browser.shortcut_config().bindings() {
        shortcut_rows = shortcut_rows.push(shortcut_binding_row(
            binding,
            browser.shortcut_capture.as_ref(),
        ));
    }

    let mut section = column![
        readable_text("Shortcuts").size(13),
        readable_text("Click a shortcut, then press the replacement keys.").size(12),
    ]
    .spacing(6);

    if let Some(capture) = &browser.shortcut_capture {
        section = section.push(shortcut_capture_feedback(capture));
    }

    let scroll_region = ScrollbarRegion::ShortcutSettings;
    let scrollbar_visibility = browser.scrollbar_visibility_for(&scroll_region);
    section = section.push(
        scrollable(smooth_scroll_content(shortcut_rows, scroll_region.clone()))
            .id(smooth_scroll_id(&scroll_region))
            .direction(auto_hide_vertical_scrollbar_direction(
                scrollbar_visibility,
                6.0,
            ))
            .style(auto_hide_scrollbar_style(scrollbar_visibility))
            .height(Length::Fixed(SHORTCUT_LIST_HEIGHT))
            .width(Length::Fill)
            .on_scroll(|_| Message::ShortcutSettingsScrolled),
    );

    section.into()
}

fn shortcut_binding_row(
    binding: &ShortcutBinding,
    capture: Option<&ShortcutCaptureState>,
) -> Element<'static, Message> {
    let is_capturing = capture.is_some_and(|capture| capture.binding_id == binding.id);
    let binding_label = if is_capturing {
        "Press keys...".to_owned()
    } else {
        binding.binding.config_value()
    };

    row![
        readable_text(binding.id.label())
            .size(12)
            .width(Length::Fill),
        button(
            container(readable_text(binding_label).size(12))
                .padding([5, 8])
                .width(Length::Fixed(SHORTCUT_BINDING_BUTTON_WIDTH)),
        )
        .on_press(Message::ShortcutCaptureStarted(binding.id))
        .style(context_menu_button_style()),
        button(readable_text("Reset").size(12))
            .on_press(Message::ShortcutBindingReset(binding.id))
            .padding([6, 10])
            .style(context_menu_button_style()),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

fn shortcut_capture_feedback(capture: &ShortcutCaptureState) -> Element<'static, Message> {
    let message = if capture.unsupported_key {
        "Unsupported shortcut. Use a letter, number, function key, arrow, or named edit key."
            .to_owned()
    } else if let Some(conflict) = capture.conflict_binding_id {
        let rejected = capture
            .rejected_binding
            .as_ref()
            .map(|binding| binding.config_value())
            .unwrap_or_else(|| "Shortcut".to_owned());
        format!("{rejected} conflicts with {}.", conflict.label())
    } else {
        format!("Listening for {}...", capture.binding_id.label())
    };

    row![
        readable_text(message).size(12).width(Length::Fill),
        button(readable_text("Cancel").size(12))
            .on_press(Message::ShortcutCaptureCanceled)
            .padding([6, 10])
            .style(context_menu_button_style()),
        Space::new().width(Length::Shrink),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}
