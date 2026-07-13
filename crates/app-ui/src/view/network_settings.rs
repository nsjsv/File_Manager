use iced::widget::{button, column, container, row, text_input, Button, Column};
use iced::{Alignment, Element, Length};

use crate::app::FileBrowser;
use crate::appearance::{context_menu_button_style, selected_sidebar_item_style};
use crate::model::Message;
use crate::typography::{localized_text, readable_text};

use super::toggle_switch::switch_control;

pub(super) fn network_settings_content(browser: &FileBrowser) -> Column<'_, Message> {
    column![
        readable_text("Network").size(20),
        readable_text("Thumbnails").size(13),
        network_thumbnail_downloads_button(browser),
        readable_text("File preview").size(13),
        max_preview_file_size_input(browser),
    ]
    .spacing(10)
    .width(Length::Fill)
}

fn network_thumbnail_downloads_button(browser: &FileBrowser) -> Button<'static, Message> {
    let enabled = browser.network_list_thumbnail_downloads_enabled();
    let label = column![
        row![
            readable_text("Network List Thumbnails")
                .size(12)
                .width(Length::Fill),
            switch_control(enabled),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        readable_text("When enabled, images and videos on network locations may use more data.")
            .size(11)
            .width(Length::Fill),
    ]
    .spacing(3);
    let label = container(label).padding([5, 8]).width(Length::Fill);
    let label = if enabled {
        label.style(selected_sidebar_item_style)
    } else {
        label
    };

    button(label)
        .on_press(Message::NetworkListThumbnailDownloadsToggled)
        .width(Length::Fill)
        .style(context_menu_button_style())
}

fn max_preview_file_size_input(browser: &FileBrowser) -> Element<'_, Message> {
    let input = text_input("3", &browser.max_preview_file_mib_input)
        .on_input(Message::MaxPreviewFileMibInputChanged)
        .on_submit(Message::MaxPreviewFileMibInputCommitted)
        .padding([6, 8])
        .size(12)
        .width(Length::Fixed(120.0));
    let save = button(container(readable_text("Save").size(12)).padding([6, 10]))
        .on_press(Message::MaxPreviewFileMibInputCommitted)
        .style(context_menu_button_style());

    let mut content = column![row![
        readable_text("Max File Preview")
            .size(12)
            .width(Length::Fill),
        input,
        readable_text("MiB").size(12),
        save,
    ]
    .spacing(8)
    .align_y(Alignment::Center)]
    .spacing(3);

    if let Some(error) = &browser.max_preview_file_mib_error {
        content = content.push(localized_text(error).size(11).width(Length::Fill));
    }

    container(content)
        .padding([5, 8])
        .width(Length::Fill)
        .into()
}
