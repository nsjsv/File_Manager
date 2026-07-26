use iced::widget::{button, container, mouse_area, row, scrollable, Column};
use iced::{Alignment, Element, Length};

use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use crate::app::FileBrowser;
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, list_panel_style,
    list_row_style, navigation_icon_button_style, navigation_text_input_style,
};
use crate::formatting::format_file_size;
use crate::icons::IconSymbol;
use crate::model::{Message, ScrollbarRegion};
use crate::typography::{localized_text, readable_text};

use super::{themed_icon, IconTone};

const SEARCH_INPUT_WIDTH: f32 = 140.0;

fn search_input_width() -> Length {
    Length::Fixed(SEARCH_INPUT_WIDTH)
}

pub(super) fn search_input_panel(browser: &FileBrowser) -> Element<'_, Message> {
    let input = iced::widget::text_input(
        &crate::localization::translate_current("Search"),
        &browser.search.input,
    )
    .on_input(Message::SearchInputChanged)
    .on_submit(Message::SearchSubmitted)
    .padding([8, 10])
    .size(15)
    .style(navigation_text_input_style)
    .width(Length::Fill);
    let content = if browser.search.input.is_empty() {
        row![input].spacing(4).align_y(Alignment::Center)
    } else {
        row![
            input,
            button(themed_icon(IconSymbol::Close, IconTone::Normal, 12.0))
                .on_press(Message::SearchCleared)
                .padding([8, 8])
                .style(navigation_icon_button_style())
        ]
        .spacing(4)
        .align_y(Alignment::Center)
    };
    container(content.width(Length::Fill))
        .width(search_input_width())
        .into()
}

pub(super) fn search_results_view(browser: &FileBrowser) -> Element<'_, Message> {
    let mut rows = Column::new().spacing(0).width(Length::Fill);
    if browser.search.results.is_empty() {
        if let Some(error) = &browser.search.error {
            rows = rows.push(search_message(error.clone()));
        } else if browser.search.is_loading {
            rows = rows.push(search_message("Searching..."));
        } else {
            rows = rows.push(search_message("No search results"));
        }
    } else {
        if let Some(error) = &browser.search.error {
            rows = rows.push(search_message(error.clone()));
        }
        for (index, hit) in browser.search.results.iter().enumerate() {
            rows = rows.push(search_result_row(hit.clone(), index));
        }
    }

    if browser.search.is_loading && !browser.search.results.is_empty() {
        rows = rows.push(search_message("Searching..."));
    }

    let region = ScrollbarRegion::SearchResults;
    let visibility = browser.scrollbar_visibility_for(&region);
    let results = scrollable(smooth_scroll_content(rows, region.clone()))
        .id(smooth_scroll_id(&region))
        .direction(auto_hide_vertical_scrollbar_direction(visibility, 8.0))
        .style(auto_hide_scrollbar_style(visibility))
        .width(Length::Fill)
        .height(Length::Fill)
        .on_scroll(|_| Message::SearchResultsScrolled);

    container(results)
        .padding([4, 6])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(list_panel_style)
        .into()
}

fn search_message(
    message: impl crate::typography::ReadableTextContent,
) -> Element<'static, Message> {
    container(localized_text(message).size(14))
        .padding([14, 12])
        .width(Length::Fill)
        .style(list_row_style(0, 0))
        .into()
}

fn search_result_row(hit: file_search::SearchHit, row_index: usize) -> Element<'static, Message> {
    let metadata = format!(
        "{} · {}",
        format_file_size(hit.size),
        hit.path
            .parent()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    let mut content = Column::new()
        .spacing(3)
        .push(readable_text(hit.display_name.clone()).size(15))
        .push(readable_text(metadata).size(12));
    if let Some(snippet) = hit.snippet.clone().filter(|snippet| !snippet.is_empty()) {
        content = content.push(readable_text(snippet).size(12));
    }

    mouse_area(
        container(content)
            .padding([8, 12])
            .width(Length::Fill)
            .height(Length::Fixed(62.0))
            .style(list_row_style(0, row_index)),
    )
    .on_press(Message::SearchResultPressed(hit))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_search_input_uses_compact_fixed_width() {
        let Length::Fixed(width) = search_input_width() else {
            panic!("navigation search input must not participate in Fill layout");
        };
        assert_eq!(width, 140.0);
    }
}
