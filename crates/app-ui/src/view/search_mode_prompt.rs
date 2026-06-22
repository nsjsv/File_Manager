use iced::widget::{column, container};
use iced::{Element, Length};

use crate::appearance::context_menu_style;
use crate::model::Message;
use crate::typography::readable_text;

use super::option_controls::action_choice_row;

const SEARCH_MODE_PROMPT_WIDTH: f32 = 460.0;

pub(super) fn search_mode_prompt_panel() -> Element<'static, Message> {
    let content = column![
        readable_text("Choose search mode").size(16),
        action_choice_row(
            "Simple Search",
            "Find file names live without building an index.",
            Message::SearchModePromptSimpleSelected,
        ),
        action_choice_row(
            "Indexed Search",
            "Build indexed paths for faster catalog, content, and media search.",
            Message::SearchModePromptIndexedSelected,
        ),
    ]
    .spacing(10)
    .width(Length::Fill);

    container(content)
        .padding(14)
        .width(Length::Fixed(SEARCH_MODE_PROMPT_WIDTH))
        .style(context_menu_style)
        .into()
}
