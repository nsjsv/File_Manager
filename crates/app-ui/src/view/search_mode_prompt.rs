use iced::widget::{button, column, container, row, Space};
use iced::{Alignment, Element, Length};

use crate::appearance::{context_menu_button_style, context_menu_style};
use crate::model::Message;
use crate::typography::readable_text;

const SEARCH_MODE_PROMPT_WIDTH: f32 = 460.0;

pub(super) fn search_mode_prompt_panel() -> Element<'static, Message> {
    let actions = row![
        Space::new().width(Length::Fill),
        button(readable_text("Use simple search").size(12))
            .on_press(Message::SearchModePromptSimpleSelected)
            .padding([6, 10])
            .style(context_menu_button_style()),
        button(readable_text("Set up indexed search").size(12))
            .on_press(Message::SearchModePromptIndexedSelected)
            .padding([6, 10])
            .style(context_menu_button_style()),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let content = column![
        readable_text("Choose search mode").size(16),
        readable_text(
            "Simple search looks up file names live and does not build or maintain an index."
        )
        .size(13),
        readable_text("Indexed search is experimental. Use simple search first.").size(13),
        readable_text(
            "Indexed search enables faster catalog searches and optional content or media indexing after you choose indexed paths."
        )
        .size(13),
        actions,
    ]
    .spacing(12)
    .width(Length::Fill);

    container(content)
        .padding(14)
        .width(Length::Fixed(SEARCH_MODE_PROMPT_WIDTH))
        .style(context_menu_style)
        .into()
}
