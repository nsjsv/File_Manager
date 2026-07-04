use file_index::{FileSearchMatch, MediaSearchKind, MediaSearchMetadata, SearchResultSource};
use iced::widget::{column, container, mouse_area, row, scrollable, text_input, Column};
use iced::{Alignment, Element, Length};

use crate::app::smooth_scroll::smooth_scroll_content;
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, path_suggestion_item_style,
    preview_panel_style, selected_path_suggestion_item_style,
};
use crate::config::SearchBackendMode;
use crate::formatting::format_middle_ellipsized_text;
use crate::icons::file_entry_icon_symbol;
use crate::model::{
    Message, ScrollbarRegion, ScrollbarVisibility, SearchMode, SearchScope, SearchState,
};
use crate::typography::readable_text;

use super::option_controls::{segmented_choice_row, SegmentedChoice};
use super::{auxiliary_window_message, themed_icon, IconTone, MENU_ICON_SIZE};

pub(crate) const SEARCH_RESULTS_HEIGHT: f32 = 320.0;
pub(crate) const SEARCH_RESULT_ROW_HEIGHT: f32 = 64.0;
pub(crate) const SEARCH_RESULT_ROW_SPACING: f32 = 4.0;
pub(crate) const SEARCH_RESULTS_PADDING: f32 = 2.0;

const SEARCH_ROOT_MAX_CHARS: usize = 64;
const SEARCH_NAME_MAX_CHARS: usize = 42;
const SEARCH_PATH_MAX_CHARS: usize = 68;

pub(crate) fn search_input_id() -> iced::widget::Id {
    iced::widget::Id::new("search-input")
}

pub(crate) fn search_results_id() -> iced::widget::Id {
    iced::widget::Id::new("search-results")
}

pub(crate) fn view_search_window(
    search: Option<&SearchState>,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    search
        .map(|search| search_panel(search, scrollbar_visibility))
        .unwrap_or_else(|| auxiliary_window_message("Search window is closed"))
}

fn search_panel(
    search: &SearchState,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    let root = search.root.to_string_lossy();
    let root = format_middle_ellipsized_text(root.as_ref(), SEARCH_ROOT_MAX_CHARS);
    let header = row![
        readable_text("Search").size(16).width(Length::Fill),
        readable_text(format!(
            "{} · {root}",
            crate::localization::translate_current(search_scope_label(search.scope))
        ))
        .size(12),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let input = text_input(
        &crate::localization::translate_current("Search files"),
        &search.query,
    )
    .id(search_input_id())
    .on_input(Message::SearchInputChanged)
    .on_submit(Message::SearchActivated)
    .padding([8, 10])
    .size(16)
    .width(Length::Fill);

    let mut content = column![header].spacing(10).width(Length::Fill);
    if let Some(notice) = search_runtime_notice(search) {
        content = content.push(notice);
    }
    content = content
        .push(search_mode_selector(
            search.mode,
            search.session_backend_mode,
        ))
        .push(input)
        .push(search_results_panel(search, scrollbar_visibility))
        .push(search_footer(search));

    container(content)
        .padding(14)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(preview_panel_style)
        .into()
}

fn search_results_panel(
    search: &SearchState,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    if search.query.trim().is_empty() {
        let message = if search.is_indexing {
            "Building the index in the background. You can search by file name now"
        } else {
            "Type to search. Press Tab to switch between current folder and Home"
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

    scrollable(smooth_scroll_content(
        matches,
        ScrollbarRegion::SearchResults,
    ))
    .id(search_results_id())
    .direction(auto_hide_vertical_scrollbar_direction(
        scrollbar_visibility,
        6.0,
    ))
    .style(auto_hide_scrollbar_style(scrollbar_visibility))
    .height(Length::Fixed(SEARCH_RESULTS_HEIGHT))
    .on_scroll(|_| Message::SearchResultsScrolled)
    .into()
}

fn search_message(message: &str) -> Element<'_, Message> {
    container(readable_text(message).size(13))
        .height(Length::Fixed(SEARCH_RESULTS_HEIGHT))
        .width(Length::Fill)
        .padding([12, 8])
        .into()
}

fn search_runtime_notice(search: &SearchState) -> Option<Element<'static, Message>> {
    let message = if search.show_index_ready_reopen_hint {
        if crate::localization::current_language_is_chinese() {
            "索引已准备，请重开搜索框"
        } else {
            "Indexed search is ready. Close and reopen Search to use it."
        }
    } else if search.indexed_fallback_session {
        if crate::localization::current_language_is_chinese() {
            "索引准备中，当前先按文件名和路径搜索"
        } else {
            "Indexed search is preparing. This window is using filename and path search for now."
        }
    } else {
        return None;
    };

    Some(
        container(readable_text(message).size(12))
            .width(Length::Fill)
            .padding([2, 0])
            .into(),
    )
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
    let mut labels = column![
        row![
            readable_text(name).size(14).width(Length::Fill),
            readable_text(search_source_label(search_match.source)).size(11),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        readable_text(path).size(12),
    ]
    .spacing(2);
    if let Some(detail) = search_match_detail(search_match) {
        labels = labels.push(readable_text(detail).size(12));
    }
    let row_content = row![
        themed_icon(
            file_entry_icon_symbol(search_match.kind, search_match.name()),
            tone,
            MENU_ICON_SIZE
        ),
        labels.width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
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
    let mode_label = crate::localization::translate_current(search_mode_label(search.mode));
    let status = if crate::localization::current_language_is_chinese() {
        if search.index_error.is_some() {
            format!("{mode_label} 索引失败 · Tab 切换范围 · Esc 关闭")
        } else if search.is_indexing {
            format!("{mode_label} 索引更新中 · Tab 切换范围 · Enter 打开结果 · Esc 关闭")
        } else if search.is_loading {
            format!("正在搜索 {mode_label} · Tab 切换范围 · Enter 打开结果 · Esc 关闭")
        } else if search.skipped_count > 0 {
            format!(
                "{mode_label} 匹配数：{} · 已跳过位置：{} · Tab 切换范围 · Enter 打开结果 · Esc 关闭",
                search.matches.len(),
                search.skipped_count
            )
        } else {
            format!(
                "{mode_label} 匹配数：{} · Tab 切换范围 · Enter 打开结果 · Esc 关闭",
                search.matches.len()
            )
        }
    } else {
        let mode_label = search_mode_label(search.mode);
        if search.index_error.is_some() {
            format!("{mode_label} index failed · Tab switches scope · Esc closes")
        } else if search.is_indexing {
            format!(
                "{mode_label} index updating · Tab switches scope · Enter opens match · Esc closes"
            )
        } else if search.is_loading {
            format!("Searching {mode_label} · Tab switches scope · Enter opens match · Esc closes")
        } else if search.skipped_count > 0 {
            format!(
                "{mode_label} matches: {} · Skipped locations: {} · Tab switches scope · Enter opens match · Esc closes",
                search.matches.len(),
                search.skipped_count
            )
        } else {
            format!(
                "{mode_label} matches: {} · Tab switches scope · Enter opens match · Esc closes",
                search.matches.len()
            )
        }
    };

    readable_text(status).size(12).into()
}

fn search_mode_selector(
    selected: SearchMode,
    search_backend_mode: SearchBackendMode,
) -> Element<'static, Message> {
    let mut choices = vec![search_mode_choice("Files", SearchMode::Files, selected)];
    if search_backend_mode == SearchBackendMode::Indexed {
        choices.push(search_mode_choice(
            "Contents",
            SearchMode::Contents,
            selected,
        ));
        choices.push(search_mode_choice("Media", SearchMode::Media, selected));
        choices.push(search_mode_choice("All", SearchMode::All, selected));
    }
    segmented_choice_row(choices)
}

fn search_mode_choice(
    label: &'static str,
    mode: SearchMode,
    selected: SearchMode,
) -> SegmentedChoice {
    SegmentedChoice {
        label,
        selected: mode == selected,
        message: Message::SearchModeSelected(mode),
    }
}

fn search_mode_label(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Files => "Files",
        SearchMode::Contents => "Contents",
        SearchMode::Media => "Media",
        SearchMode::All => "All",
    }
}

fn search_source_label(source: SearchResultSource) -> &'static str {
    match source {
        SearchResultSource::Files => "Files",
        SearchResultSource::Contents => "Contents",
        SearchResultSource::Media => "Media",
    }
}

fn search_match_detail(search_match: &FileSearchMatch) -> Option<String> {
    search_match
        .snippet
        .as_deref()
        .map(|snippet| format_middle_ellipsized_text(snippet.trim(), SEARCH_PATH_MAX_CHARS))
        .filter(|snippet| !snippet.is_empty())
        .or_else(|| search_match.media.as_ref().map(media_detail))
}

fn media_detail(media: &MediaSearchMetadata) -> String {
    let mut parts = Vec::new();
    parts.push(match media.media_kind {
        MediaSearchKind::Image => crate::localization::translate_current("Image"),
        MediaSearchKind::Audio => crate::localization::translate_current("Audio"),
        MediaSearchKind::Video => crate::localization::translate_current("Video"),
    });
    if let (Some(width), Some(height)) = (media.width, media.height) {
        parts.push(format!("{width}x{height}"));
    }
    if let Some(duration_ms) = media.duration_ms {
        parts.push(format_duration_ms(duration_ms));
    }
    if let Some(codec) = media.codec.as_deref().filter(|codec| !codec.is_empty()) {
        parts.push(codec.to_owned());
    }
    if let Some(exif) = media.exif.first() {
        parts.push(format!("{}: {}", exif.tag, exif.value));
    }
    parts.join(" · ")
}

fn format_duration_ms(duration_ms: u64) -> String {
    let total_seconds = duration_ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

fn search_scope_label(scope: SearchScope) -> &'static str {
    match scope {
        SearchScope::CurrentDirectory => "Current Folder",
        SearchScope::HomeDirectory => "Home",
    }
}
