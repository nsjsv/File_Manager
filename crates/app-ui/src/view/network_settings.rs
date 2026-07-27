use iced::widget::{button, column, container, row, text_input};
use iced::{Alignment, Element, Length};

use crate::app::FileBrowser;
use crate::appearance::context_menu_button_style;
use crate::model::Message;
use crate::typography::{localized_text, readable_text};

use super::settings_group::{info_setting_row, toggle_setting_row};

pub(super) fn network_thumbnails_row(browser: &FileBrowser) -> Element<'static, Message> {
    toggle_setting_row(
        "Remote List Thumbnails",
        Some("When enabled, images and videos on remote locations may use more data."),
        browser.remote_list_thumbnail_downloads_enabled(),
        Message::NetworkListThumbnailDownloadsToggled,
    )
}

pub(super) fn max_preview_file_size_row(browser: &FileBrowser) -> Element<'_, Message> {
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

    info_setting_row(content.into())
}
