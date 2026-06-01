use file_core::FileSearchMatch;
use iced::widget::{column, container, mouse_area, row, scrollable, text_input, Column};
use iced::{Alignment, Element, Length};

use crate::appearance::{
    path_suggestion_item_style, preview_panel_style, selected_path_suggestion_item_style,
};
use crate::formatting::format_middle_ellipsized_text;
use crate::icons::file_entry_icon_symbol;
use crate::model::{Message, SearchScope, SearchState};
use crate::typography::readable_text;

use super::{auxiliary_window_message, themed_icon, IconTone, MENU_ICON_SIZE};

pub(crate) const SEARCH_RESULTS_HEIGHT: f32 = 320.0;
pub(crate) const SEARCH_RESULT_ROW_HEIGHT: f32 = 54.0;
pub(crate) const SEARCH_RESULT_ROW_SPACING: f32 = 4.0;
pub(crate) const SEARCH_RESULTS_PADDING: f32 = 2.0;

const SEARCH_ROOT_MAX_CHARS: usize = 64;
const SEARCH_NAME_MAX_CHARS: usize = 42;
const SEARCH_PATH_MAX_CHARS: usize = 68;

pub(crate) fn search_input_id() -> text_input::Id {
    text_input::Id::new("search-input")
}

pub(crate) fn search_results_id() -> scrollable::Id {
    scrollable::Id::new("search-results")
}

pub(crate) fn view_search_window(search: Option<&SearchState>) -> Element<'_, Message> {
    search
        .map(search_panel)
        .unwrap_or_else(|| auxiliary_window_message("Search window is closed"))
}

fn search_panel(search: &SearchState) -> Element<'_, Message> {
    let root = search.root.to_string_lossy();
    let root = format_middle_ellipsized_text(root.as_ref(), SEARCH_ROOT_MAX_CHARS);
    let header = row![
        readable_text("Search").size(16).width(Length::Fill),
        readable_text(format!("{} · {root}", search_scope_label(search.scope))).size(12),
    ]
    .spacing(10)
    .align_items(Alignment::Center);

    let input = text_input("Search files", &search.query)
        .id(search_input_id())
        .on_input(Message::SearchInputChanged)
        .on_submit(Message::SearchActivated)
        .padding([8, 10])
        .size(16)
        .width(Length::Fill);

    let content = column![
        header,
        input,
        search_results_panel(search),
        search_footer(search)
    ]
    .spacing(10)
    .width(Length::Fill);

    container(content)
        .padding(14)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(preview_panel_style)
        .into()
}

fn search_results_panel(search: &SearchState) -> Element<'_, Message> {
    if search.query.trim().is_empty() {
        let message = if search.is_indexing {
            "Building the index in the background. You can search by file name now"
        } else {
            "Type a file name to search. Press Tab to switch between current folder and Home"
        };
        return search_message(message);
    }

    if let Some(error) = search.error.as_deref() {
        return search_message(error);
    }

    if let Some(error) = search.index_error.as_deref() {
        return search_message(error);
    }

    if search.is_indexing && search.matches.is_empty() {
        return search_message("Building index. Search will run automatically when ready...");
    }

    if search.is_loading && search.matches.is_empty() {
        return search_message("Searching...");
    }

    if search.matches.is_empty() {
        return search_message("No matches");
    }

    let mut matches = Column::new()
        .spacing(SEARCH_RESULT_ROW_SPACING)
        .padding(SEARCH_RESULTS_PADDING);
    for (index, search_match) in search.matches.iter().enumerate() {
        matches = matches.push(search_match_row(
            search_match,
            search.selected_match == Some(index),
        ));
    }

    scrollable(matches)
        .id(search_results_id())
        .direction(iced::widget::scrollable::Direction::Vertical(
            iced::widget::scrollable::Properties::new()
                .width(6.0)
                .scroller_width(6.0),
        ))
        .height(Length::Fixed(SEARCH_RESULTS_HEIGHT))
        .into()
}

fn search_message(message: &str) -> Element<'_, Message> {
    container(readable_text(message).size(13))
        .height(Length::Fixed(SEARCH_RESULTS_HEIGHT))
        .width(Length::Fill)
        .padding([12, 8])
        .into()
}

fn search_match_row(search_match: &FileSearchMatch, is_selected: bool) -> Element<'_, Message> {
    let tone = if is_selected {
        IconTone::Selected
    } else {
        IconTone::Normal
    };
    let name = search_match.name().to_string_lossy();
    let name = format_middle_ellipsized_text(name.as_ref(), SEARCH_NAME_MAX_CHARS);
    let path = search_match.relative_path.to_string_lossy();
    let path = format_middle_ellipsized_text(path.as_ref(), SEARCH_PATH_MAX_CHARS);
    let labels = column![readable_text(name).size(14), readable_text(path).size(12),].spacing(2);
    let row_content = row![
        themed_icon(
            file_entry_icon_symbol(search_match.kind, search_match.name()),
            tone,
            MENU_ICON_SIZE
        ),
        labels.width(Length::Fill),
    ]
    .spacing(8)
    .align_items(Alignment::Center);
    let item = container(row_content)
        .padding([6, 8])
        .height(Length::Fixed(SEARCH_RESULT_ROW_HEIGHT))
        .width(Length::Fill);
    let item = if is_selected {
        item.style(selected_path_suggestion_item_style)
    } else {
        item.style(path_suggestion_item_style)
    };

    mouse_area(item)
        .on_press(Message::SearchMatchSelected(search_match.path.clone()))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

fn search_footer(search: &SearchState) -> Element<'_, Message> {
    let status = if search.index_error.is_some() {
        "Index build failed · Tab switches scope · Esc closes".to_owned()
    } else if search.is_indexing {
        "Index updating · Tab switches scope · Enter opens match · Esc closes".to_owned()
    } else if search.is_loading {
        "Searching · Tab switches scope · Enter opens match · Esc closes".to_owned()
    } else if search.skipped_count > 0 {
        format!(
            "Matches: {} · Skipped locations: {} · Tab switches scope · Enter opens match · Esc closes",
            search.matches.len(),
            search.skipped_count
        )
    } else {
        format!(
            "Matches: {} · Tab switches scope · Enter opens match · Esc closes",
            search.matches.len()
        )
    };

    readable_text(status).size(12).into()
}

fn search_scope_label(scope: SearchScope) -> &'static str {
    match scope {
        SearchScope::CurrentDirectory => "Current Folder",
        SearchScope::HomeDirectory => "Home",
    }
}
