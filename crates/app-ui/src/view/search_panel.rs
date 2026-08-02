use std::fmt;

use file_search::SearchFileKind;
use iced::widget::{
    button, container, mouse_area, pick_list, responsive, row, scrollable, tooltip, Column,
};
use iced::{Alignment, Element, Length};

use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use crate::app::FileBrowser;
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, context_menu_style,
    list_panel_style, list_row_style, navigation_icon_button_style, navigation_text_input_style,
    selected_row_style,
};
use crate::formatting::{format_file_size, format_middle_ellipsized_text};
use crate::icons::IconSymbol;
use crate::model::{
    Message, ModifiedTimePreset, ScrollbarRegion, SearchContentCategory, SearchObjectType,
    SearchResultCompletion,
};
use crate::typography::{localized_text, readable_text};

use super::{themed_icon, IconTone};

const SEARCH_INPUT_WIDTH: f32 = 140.0;
const SEARCH_FILTER_WIDTH: f32 = 116.0;
const WIDE_SEARCH_TOOLBAR_HEIGHT: f32 = 108.0;
const NARROW_SEARCH_TOOLBAR_HEIGHT: f32 = 158.0;
const NARROW_SEARCH_TOOLBAR_WIDTH: f32 = 390.0;
pub(crate) const SEARCH_RESULT_ROW_HEIGHT: f32 = 78.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SearchObjectTypeOption(SearchObjectType);

impl fmt::Display for SearchObjectTypeOption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&crate::localization::translate_current(self.0.label()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SearchContentCategoryOption(SearchContentCategory);

impl fmt::Display for SearchContentCategoryOption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&crate::localization::translate_current(self.0.label()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModifiedTimePresetOption(ModifiedTimePreset);

impl fmt::Display for ModifiedTimePresetOption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&crate::localization::translate_current(self.0.label()))
    }
}

fn search_input_width() -> Length {
    Length::Fixed(SEARCH_INPUT_WIDTH)
}

pub(super) fn search_input_panel(browser: &FileBrowser) -> Element<'_, Message> {
    let input_value = browser
        .search_workspace
        .as_ref()
        .map(|workspace| workspace.input.as_str())
        .unwrap_or_default();
    let input = iced::widget::text_input(
        &crate::localization::translate_current("Search"),
        input_value,
    )
    .on_input(Message::SearchInputChanged)
    .on_submit(Message::SearchSubmitted)
    .padding([8, 10])
    .size(15)
    .style(navigation_text_input_style)
    .width(Length::Fill);
    let content = if input_value.is_empty() {
        row![input].spacing(4).align_y(Alignment::Center)
    } else {
        row![
            input,
            tooltip(
                button(themed_icon(IconSymbol::Close, IconTone::Normal, 12.0))
                    .on_press(Message::SearchKeywordCleared)
                    .padding([8, 8])
                    .style(navigation_icon_button_style()),
                tooltip_label("Clear search text"),
                tooltip::Position::Bottom,
            )
        ]
        .spacing(4)
        .align_y(Alignment::Center)
    };
    container(content.width(Length::Fill))
        .width(search_input_width())
        .into()
}

pub(super) fn search_results_view(browser: &FileBrowser) -> Element<'_, Message> {
    let Some(workspace) = browser.search_workspace.as_ref() else {
        return container(readable_text("")).into();
    };
    let mut rows = Column::new().spacing(0).width(Length::Fill);
    if let Some(failure) = &workspace.window.failure {
        rows = rows.push(search_message(failure.clone()));
    }
    for (index, hit) in workspace.window.hits.iter().enumerate() {
        rows = rows.push(search_result_row(
            hit.clone(),
            index,
            workspace.selection.is_selected(&hit.path),
            workspace.selection.focused_path() == Some(hit.path.as_path()),
        ));
    }
    if workspace.window.is_loading {
        rows = rows.push(search_message("Searching..."));
    } else if workspace.window.hits.is_empty() && workspace.window.failure.is_none() {
        rows = rows.push(search_message("No search results"));
    }
    if let Some(completion) = workspace.window.completion {
        rows = rows.push(search_completion_message(completion));
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

    Column::new()
        .push(search_workspace_toolbar(browser))
        .push(
            container(results)
                .padding([4, 6])
                .width(Length::Fill)
                .height(Length::Fill)
                .style(list_panel_style),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn search_workspace_toolbar(browser: &FileBrowser) -> Element<'_, Message> {
    let workspace = browser
        .search_workspace
        .as_ref()
        .expect("search toolbar requires an active workspace");
    let root_label = format!(
        "Search in {}",
        format_middle_ellipsized_text(&workspace.root.path().to_string_lossy(), 72)
    );
    let object_type = workspace.filters.object_type;
    let content_category = workspace.filters.content_category;
    let modified_time = workspace.filters.modified_time;
    let selected = browser.active_search_selection().unwrap_or_default();

    responsive(move |viewport_size| {
        search_workspace_toolbar_layout(
            root_label.clone(),
            object_type,
            content_category,
            modified_time,
            selected.clone(),
            viewport_size.width < NARROW_SEARCH_TOOLBAR_WIDTH,
        )
    })
    .width(Length::Fill)
    .height(Length::Shrink)
    .into()
}

fn search_workspace_toolbar_layout(
    root_label: String,
    object_type: SearchObjectType,
    content_category: SearchContentCategory,
    modified_time: ModifiedTimePreset,
    selected: Vec<std::path::PathBuf>,
    narrow: bool,
) -> Element<'static, Message> {
    let header = row![
        themed_icon(IconSymbol::Search, IconTone::Normal, 15.0),
        localized_text(root_label).size(13).width(Length::Fill),
        tooltip(
            button(themed_icon(IconSymbol::Close, IconTone::Normal, 13.0))
                .on_press(Message::SearchWorkspaceClosed)
                .padding([6, 8])
                .style(navigation_icon_button_style()),
            tooltip_label("Close search"),
            tooltip::Position::Bottom,
        )
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let filters: Element<'static, Message> = if narrow {
        Column::new()
            .push(
                row![
                    object_type_filter(object_type, Length::Fill),
                    content_category_filter(content_category, Length::Fill),
                ]
                .spacing(6),
            )
            .push(modified_time_filter(modified_time, Length::Fill))
            .spacing(5)
            .into()
    } else {
        row![
            object_type_filter(object_type, Length::Fixed(SEARCH_FILTER_WIDTH)),
            content_category_filter(content_category, Length::Fixed(SEARCH_FILTER_WIDTH)),
            modified_time_filter(modified_time, Length::Fixed(SEARCH_FILTER_WIDTH)),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    };

    container(
        Column::new()
            .push(header)
            .push(filters)
            .push(search_selection_actions(selected))
            .spacing(6),
    )
    .padding([7, 10])
    .width(Length::Fill)
    .height(Length::Fixed(if narrow {
        NARROW_SEARCH_TOOLBAR_HEIGHT
    } else {
        WIDE_SEARCH_TOOLBAR_HEIGHT
    }))
    .align_y(Alignment::Center)
    .style(list_panel_style)
    .into()
}

fn object_type_filter(selected: SearchObjectType, width: Length) -> Element<'static, Message> {
    pick_list(
        SearchObjectType::ALL.map(SearchObjectTypeOption),
        Some(SearchObjectTypeOption(selected)),
        |selected| Message::SearchObjectTypeSelected(selected.0),
    )
    .width(width)
    .text_size(12)
    .padding([5, 6])
    .into()
}

fn content_category_filter(
    selected: SearchContentCategory,
    width: Length,
) -> Element<'static, Message> {
    pick_list(
        SearchContentCategory::ALL.map(SearchContentCategoryOption),
        Some(SearchContentCategoryOption(selected)),
        |selected| Message::SearchContentCategorySelected(selected.0),
    )
    .width(width)
    .text_size(12)
    .padding([5, 6])
    .into()
}

fn modified_time_filter(selected: ModifiedTimePreset, width: Length) -> Element<'static, Message> {
    pick_list(
        ModifiedTimePreset::ALL.map(ModifiedTimePresetOption),
        Some(ModifiedTimePresetOption(selected)),
        |selected| Message::SearchModifiedTimeSelected(selected.0),
    )
    .width(width)
    .text_size(12)
    .padding([5, 6])
    .into()
}

fn search_selection_actions(selected: Vec<std::path::PathBuf>) -> Element<'static, Message> {
    if selected.is_empty() {
        return container(readable_text("")).width(Length::Fill).into();
    }

    let mut actions = row![
        search_action_button(IconSymbol::Copy, "Copy", Message::CopySelected),
        search_action_button(IconSymbol::ArrowRight, "Cut", Message::MoveSelected),
        search_action_button(IconSymbol::Trash, "Trash", Message::TrashSelected),
        search_action_button(
            IconSymbol::TriangleAlert,
            "Delete",
            Message::SearchDeletePermanentlySelected,
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    if let [path] = selected.as_slice() {
        actions = actions.push(search_action_button(
            IconSymbol::FolderOpen,
            "Open folder",
            Message::SearchOpenContainingDirectory(path.clone()),
        ));
    }
    container(actions)
        .width(Length::Fill)
        .align_x(Alignment::End)
        .into()
}

fn search_action_button(
    icon: IconSymbol,
    label: &'static str,
    message: Message,
) -> Element<'static, Message> {
    tooltip(
        button(themed_icon(icon, IconTone::Normal, 13.0))
            .on_press(message)
            .width(Length::Fixed(30.0))
            .height(Length::Fixed(28.0))
            .padding([5, 7])
            .style(navigation_icon_button_style()),
        tooltip_label(label),
        tooltip::Position::Bottom,
    )
    .into()
}

fn search_completion_message(completion: SearchResultCompletion) -> Element<'static, Message> {
    match completion {
        SearchResultCompletion::Complete => container(readable_text("")).into(),
        SearchResultCompletion::Truncated => search_message("Showing the first 100 results"),
        SearchResultCompletion::Partial { inspected_entries } => search_message(format!(
            "Partial results after inspecting {inspected_entries} entries"
        )),
    }
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

fn search_result_row(
    hit: file_search::SearchHit,
    row_index: usize,
    selected: bool,
    focused: bool,
) -> Element<'static, Message> {
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
    let result_icon = match hit.kind {
        SearchFileKind::File => IconSymbol::File,
        SearchFileKind::Directory => IconSymbol::Folder,
        SearchFileKind::Symlink => IconSymbol::Link,
        SearchFileKind::Other => IconSymbol::File,
    };
    let path = hit.path;
    let content = row![themed_icon(result_icon, IconTone::Normal, 18.0), content]
        .spacing(10)
        .align_y(Alignment::Center);
    let row = container(content)
        .padding([8, 12])
        .width(Length::Fill)
        .height(Length::Fixed(SEARCH_RESULT_ROW_HEIGHT));
    let row = if selected || focused {
        row.style(selected_row_style)
    } else {
        row.style(list_row_style(0, row_index))
    };

    mouse_area(row)
        .on_press(Message::SearchResultPressed(path.clone()))
        .on_right_press(Message::SearchResultRightClicked(path))
        .into()
}

fn tooltip_label(label: &'static str) -> Element<'static, Message> {
    container(localized_text(label).size(11))
        .padding([5, 7])
        .style(context_menu_style)
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
