use iced::widget::{column, container, row, Space};
use iced::{Element, Length};

use crate::appearance::context_menu_style;
use crate::config::SearchBackendMode;
use crate::model::{Message, SearchModePromptState};
use crate::typography::readable_text;

use super::option_controls::{
    inactive_primary_action_button, primary_action_button, selectable_choice_row,
};

const SEARCH_MODE_PROMPT_WIDTH: f32 = 460.0;

pub(super) fn search_mode_prompt_panel(
    prompt: &SearchModePromptState,
) -> Element<'static, Message> {
    let next_button = if prompt.selected_mode.is_some() {
        primary_action_button("Next", Message::SearchModePromptNextPressed)
    } else {
        inactive_primary_action_button("Next")
    };
    let actions = row![Space::new().width(Length::Fill), next_button]
        .spacing(6)
        .width(Length::Fill);

    let content = column![
        readable_text("Choose search mode").size(16),
        selectable_choice_row(
            "Simple Search",
            "Find file names live without building an index.",
            prompt.selected_mode == Some(SearchBackendMode::Simple),
            Message::SearchModePromptModeSelected(SearchBackendMode::Simple),
        ),
        selectable_choice_row(
            "Indexed Search",
            "Build indexed paths for faster catalog, content, and media search.",
            prompt.selected_mode == Some(SearchBackendMode::Indexed),
            Message::SearchModePromptModeSelected(SearchBackendMode::Indexed),
        ),
        actions,
    ]
    .spacing(10)
    .width(Length::Fill);

    container(content)
        .padding(14)
        .width(Length::Fixed(SEARCH_MODE_PROMPT_WIDTH))
        .style(context_menu_style)
        .into()
}
