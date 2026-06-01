use std::path::Path;
use std::time::SystemTime;

use file_core::{DirectoryEntry, FileKind, FileSearchMatch};
use iced::widget::{
    button, column, container, image, mouse_area, row, scrollable, text, text_input, Button,
    Column, Row, Space, Svg,
};
use iced::{Alignment, Element, Length, Point, Theme};

use crate::anchored_popup::anchored_popup;
use crate::app::FileBrowser;
use crate::appearance::{
    app_content_style, context_menu_button_style, context_menu_style, drag_preview_style,
    error_notification_style, hovered_sidebar_item_style, icon_svg_style,
    navigation_icon_button_style, path_suggestion_item_style, path_suggestions_style,
    preview_panel_style, selected_icon_svg_style, selected_path_suggestion_item_style,
    selected_sidebar_item_style, sidebar_style, switch_thumb_style, switch_track_off_style,
    switch_track_on_style, warning_icon_svg_style,
};
use crate::config::COLUMN_FIXED_COUNT_OPTIONS;
use crate::floating_surface::{
    dismissable_floating_surface, floating_surface, FloatingContent, FloatingPlacement,
};
use crate::formatting::{format_file_size, format_middle_ellipsized_text};
use crate::icons::{file_entry_icon_symbol, preview_entry_icon_symbol, IconSymbol};
use crate::model::{
    trash_location_path, ColumnViewMode, ContextMenuState, Message, PreviewArchiveEntry,
    PreviewContent, PreviewSize, PreviewState, SearchScope, SearchState, SidebarLocation,
    TransferConflictChoice, TransferConflictItem, TransferConflictMetadata, TransferConflictState,
    TRASH_LOCATION_LABEL,
};
use crate::operation_queue_view::{
    operation_queue_indicator, operation_queue_panel, OPERATION_QUEUE_INDICATOR_BOTTOM,
    OPERATION_QUEUE_INDICATOR_RIGHT, OPERATION_QUEUE_PANEL_BOTTOM,
};
use crate::preview::PREVIEW_TEXT_LIMIT;
use crate::selection_marquee::selection_marquee_overlay;
use crate::sidebar::SIDEBAR_WIDTH;
use crate::three_column_view::column_browser_view;
use crate::typography::readable_text;

const PREVIEW_HEADER_RESERVED_HEIGHT: f32 = 96.0;
const PREVIEW_MIN_SCROLL_HEIGHT: f32 = 160.0;
const TOOLBAR_ICON_SIZE: f32 = 16.0;
const TAB_ICON_SIZE: f32 = 14.0;
const TAB_CLOSE_ICON_SIZE: f32 = 12.0;
const MENU_ICON_SIZE: f32 = 16.0;
const PREVIEW_ICON_SIZE: f32 = 16.0;
const TAB_LABEL_MAX_CHARS: usize = 24;
const SIDEBAR_LABEL_MAX_CHARS: usize = 22;
const PATH_SUGGESTION_MAX_CHARS: usize = 72;
pub(crate) const SEARCH_RESULTS_HEIGHT: f32 = 320.0;
pub(crate) const SEARCH_RESULT_ROW_HEIGHT: f32 = 54.0;
pub(crate) const SEARCH_RESULT_ROW_SPACING: f32 = 4.0;
pub(crate) const SEARCH_RESULTS_PADDING: f32 = 2.0;
const SEARCH_ROOT_MAX_CHARS: usize = 64;
const SEARCH_NAME_MAX_CHARS: usize = 42;
const SEARCH_PATH_MAX_CHARS: usize = 68;
const COLUMN_SETTINGS_FLOAT_WIDTH: f32 = 260.0;
const ERROR_NOTIFICATION_FLOAT_WIDTH: f32 = 560.0;
const ERROR_NOTIFICATION_FLOAT_X: f32 = SIDEBAR_WIDTH + 18.0;
const ERROR_NOTIFICATION_FLOAT_Y: f32 = 18.0;
const ERROR_NOTIFICATION_MAX_CHARS: usize = 96;
const TRANSFER_CONFLICT_PANEL_WIDTH: f32 = 560.0;
const TRANSFER_CONFLICT_PATH_MAX_CHARS: usize = 68;
const PREVIEW_PATH_MAX_CHARS: usize = 64;
const PREVIEW_ENTRY_NAME_MAX_CHARS: usize = 48;
const PREVIEW_ARCHIVE_INDENT_WIDTH: f32 = 18.0;
const PREVIEW_ARCHIVE_TOGGLE_WIDTH: f32 = 16.0;
const DRAG_PREVIEW_ICON_SIZE: f32 = 18.0;
const DRAG_PREVIEW_LABEL_MAX_CHARS: usize = 34;
const DRAG_PREVIEW_OFFSET_X: f32 = 14.0;
const DRAG_PREVIEW_OFFSET_Y: f32 = 14.0;

pub(crate) fn rename_input_id() -> text_input::Id {
    text_input::Id::new("rename-input")
}

pub(crate) fn path_input_id() -> text_input::Id {
    text_input::Id::new("path-input")
}

pub(crate) fn search_input_id() -> text_input::Id {
    text_input::Id::new("search-input")
}

pub(crate) fn search_results_id() -> scrollable::Id {
    scrollable::Id::new("search-results")
}

pub(crate) fn column_browser_scroll_id() -> scrollable::Id {
    scrollable::Id::new("column-browser")
}

pub(crate) fn view_search_window(search: Option<&SearchState>) -> Element<'_, Message> {
    search
        .map(search_panel)
        .unwrap_or_else(|| auxiliary_window_message("Search window is closed"))
}

pub(crate) fn view_preview_window(
    preview: Option<&PreviewState>,
    size: PreviewSize,
) -> Element<'_, Message> {
    preview
        .map(|preview| preview_panel(preview, size))
        .unwrap_or_else(|| {
            auxiliary_window_message("Select a file and press Space to load preview")
        })
}

fn auxiliary_window_message(message: &'static str) -> Element<'static, Message> {
    container(readable_text(message).size(14))
        .padding(18)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(app_content_style)
        .into()
}

pub(crate) fn view_browser(browser: &FileBrowser) -> Element<'_, Message> {
    let tabs = tab_bar(browser);
    let navigation_bar = row![
        navigation_icon_button(IconSymbol::ArrowLeft, Message::Back),
        navigation_icon_button(IconSymbol::ArrowRight, Message::Forward),
        navigation_icon_button(IconSymbol::ArrowUp, Message::Up),
        path_input_panel(browser),
    ]
    .spacing(8)
    .align_items(Alignment::Start);

    let header_content = column![tabs, navigation_bar]
        .spacing(14)
        .padding(18)
        .width(Length::Fill);

    let main_content = column![header_content, column_browser_view(browser)]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill);

    let content = row![
        sidebar_view(browser),
        container(main_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(app_content_style),
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    let mut floating = Vec::new();
    let mut dismiss_on_outside = false;
    if let Some(conflict) = &browser.transfer_conflict {
        floating.push(FloatingContent {
            element: transfer_conflict_panel(conflict),
            placement: FloatingPlacement::Center,
        });
    } else if let Some(marquee) = &browser.selection_marquee {
        floating.push(selection_marquee_overlay(marquee));
    } else if let Some(drag_preview) = drag_preview_panel(browser) {
        floating.push(FloatingContent {
            element: drag_preview,
            placement: FloatingPlacement::Free(drag_preview_position(browser.cursor_position)),
        });
    } else if let Some(context_menu) = &browser.context_menu {
        dismiss_on_outside = true;
        floating.push(FloatingContent {
            element: context_menu_panel(context_menu, browser.is_trash_view),
            placement: FloatingPlacement::At(context_menu.position),
        });
    } else if browser.is_column_view_settings_open {
        dismiss_on_outside = true;
        floating.push(FloatingContent {
            element: column_settings_panel(browser),
            placement: FloatingPlacement::Center,
        });
    }

    if let Some(error) = browser.error.as_deref() {
        floating.push(FloatingContent {
            element: error_notification_panel(error),
            placement: FloatingPlacement::At(iced::Point::new(
                ERROR_NOTIFICATION_FLOAT_X,
                ERROR_NOTIFICATION_FLOAT_Y,
            )),
        });
    }

    if browser.operation_queue.is_panel_open() {
        dismiss_on_outside = true;
        floating.push(FloatingContent {
            element: operation_queue_panel(&browser.operation_queue),
            placement: FloatingPlacement::BottomLeft {
                left: SIDEBAR_WIDTH + 12.0,
                bottom: OPERATION_QUEUE_PANEL_BOTTOM,
            },
        });
    }

    if let Some(indicator) = operation_queue_indicator(&browser.operation_queue) {
        floating.push(FloatingContent {
            element: indicator,
            placement: FloatingPlacement::BottomRightInArea {
                area_width: SIDEBAR_WIDTH,
                right: OPERATION_QUEUE_INDICATOR_RIGHT,
                bottom: OPERATION_QUEUE_INDICATOR_BOTTOM,
            },
        });
    }

    if dismiss_on_outside {
        dismissable_floating_surface(content, floating, Message::DismissFloating)
    } else {
        floating_surface(content, floating)
    }
}

fn drag_preview_position(cursor_position: Point) -> Point {
    Point::new(
        cursor_position.x + DRAG_PREVIEW_OFFSET_X,
        cursor_position.y + DRAG_PREVIEW_OFFSET_Y,
    )
}

fn drag_preview_panel(browser: &FileBrowser) -> Option<Element<'_, Message>> {
    let drag = browser.file_drag.as_ref()?;
    if !drag.is_dragging() {
        return None;
    }
    let source = drag.sources.first()?;
    let (symbol, tone, label) = drag_preview_item(browser, source);
    let label = format_middle_ellipsized_text(&label, DRAG_PREVIEW_LABEL_MAX_CHARS);
    let content = row![
        themed_icon(symbol, tone, DRAG_PREVIEW_ICON_SIZE),
        readable_text(label).size(13),
    ]
    .spacing(8)
    .align_items(Alignment::Center);

    Some(
        container(content)
            .padding([7, 10])
            .style(drag_preview_style)
            .into(),
    )
}

fn drag_preview_item(browser: &FileBrowser, path: &Path) -> (IconSymbol, IconTone, String) {
    if let Some(entry) = browser.entry_for_path(path) {
        return drag_preview_entry_item(entry);
    }

    let name = path.file_name().unwrap_or_else(|| path.as_os_str());
    (
        file_entry_icon_symbol(FileKind::Other, name),
        IconTone::Normal,
        name.to_string_lossy().into_owned(),
    )
}

fn drag_preview_entry_item(entry: &DirectoryEntry) -> (IconSymbol, IconTone, String) {
    let symbol = if entry.kind == FileKind::Symlink && entry.is_broken_symlink {
        IconSymbol::TriangleAlert
    } else {
        file_entry_icon_symbol(entry.kind, entry.name())
    };
    let tone = if symbol == IconSymbol::TriangleAlert {
        IconTone::Warning
    } else {
        IconTone::Normal
    };

    (symbol, tone, entry.name().to_string_lossy().into_owned())
}

fn navigation_icon_button(icon: IconSymbol, message: Message) -> Button<'static, Message> {
    button(themed_icon(icon, IconTone::Normal, TOOLBAR_ICON_SIZE))
        .on_press(message)
        .padding([6, 8])
        .style(navigation_icon_button_style())
}

fn tab_bar(browser: &FileBrowser) -> Element<'_, Message> {
    let mut tabs = Row::new().spacing(6).align_items(Alignment::Center);
    for tab in &browser.tabs {
        tabs = tabs.push(tab_button(
            tab.id,
            tab.directory.as_path(),
            tab.is_trash_view,
            tab.id == browser.active_tab_id,
        ));
    }

    container(
        scrollable(tabs)
            .direction(iced::widget::scrollable::Direction::Horizontal(
                iced::widget::scrollable::Properties::new()
                    .width(6.0)
                    .scroller_width(6.0),
            ))
            .height(Length::Shrink)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .into()
}

fn tab_button<'a>(
    tab_id: usize,
    directory: &'a Path,
    is_trash_view: bool,
    is_active: bool,
) -> Element<'a, Message> {
    let tone = if is_active {
        IconTone::Selected
    } else {
        IconTone::Normal
    };
    let symbol = if is_trash_view {
        IconSymbol::Trash
    } else {
        IconSymbol::Folder
    };
    let label = row![
        themed_icon(symbol, tone, TAB_ICON_SIZE),
        readable_text(tab_title(directory, is_trash_view)).size(13),
        button(themed_icon(IconSymbol::Close, tone, TAB_CLOSE_ICON_SIZE))
            .on_press(Message::TabCloseRequested(tab_id))
            .padding([2, 2])
            .style(navigation_icon_button_style()),
    ]
    .spacing(6)
    .align_items(Alignment::Center);

    let tab = container(label).padding([4, 8]);
    let tab = if is_active {
        tab.style(selected_sidebar_item_style)
    } else {
        tab
    };

    mouse_area(tab)
        .on_press(Message::TabPressed(tab_id))
        .on_middle_press(Message::TabCloseRequested(tab_id))
        .on_enter(Message::TabDragEntered(tab_id))
        .on_release(Message::TabDragFinished)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

fn tab_title(directory: &Path, is_trash_view: bool) -> String {
    if is_trash_view {
        return TRASH_LOCATION_LABEL.to_owned();
    }

    let title = directory
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| directory.to_string_lossy().into_owned());
    format_middle_ellipsized_text(&title, TAB_LABEL_MAX_CHARS)
}

fn path_input_panel(browser: &FileBrowser) -> Element<'_, Message> {
    if browser.is_trash_view {
        return container(
            row![
                themed_icon(IconSymbol::Trash, IconTone::Normal, TOOLBAR_ICON_SIZE),
                readable_text(TRASH_LOCATION_LABEL).size(16),
            ]
            .spacing(8)
            .align_items(Alignment::Center),
        )
        .padding([7, 10])
        .width(Length::Fill)
        .into();
    }

    let input = text_input("Path", &browser.path_input)
        .id(path_input_id())
        .on_input(Message::PathInputChanged)
        .on_submit(Message::PathInputSubmitted)
        .padding([7, 10])
        .size(16)
        .width(Length::Fill);

    let popup = (!browser.path_suggestions.is_empty()).then(|| path_suggestions_panel(browser));

    container(anchored_popup(input, popup))
        .width(Length::Fill)
        .into()
}

fn path_suggestions_panel(browser: &FileBrowser) -> Element<'_, Message> {
    let mut suggestions = Column::new().spacing(3).padding(4);
    for (index, suggestion) in browser.path_suggestions.iter().enumerate() {
        suggestions = suggestions.push(path_suggestion_row(
            suggestion,
            browser.path_suggestion_selection == Some(index),
        ));
    }

    container(suggestions)
        .width(Length::Fill)
        .style(path_suggestions_style)
        .into()
}

fn path_suggestion_row(path: &std::path::PathBuf, is_selected: bool) -> Element<'_, Message> {
    let label = path.to_string_lossy();
    let label = format_middle_ellipsized_text(label.as_ref(), PATH_SUGGESTION_MAX_CHARS);
    let item = container(readable_text(label).size(13).width(Length::Fill))
        .padding([5, 8])
        .width(Length::Fill);
    let item = if is_selected {
        item.style(selected_path_suggestion_item_style)
    } else {
        item.style(path_suggestion_item_style)
    };

    mouse_area(item)
        .on_press(Message::PathSuggestionSelected(path.clone()))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
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

fn error_notification_panel(error: &str) -> Element<'_, Message> {
    let message = format_middle_ellipsized_text(error, ERROR_NOTIFICATION_MAX_CHARS);
    let content = row![
        themed_icon(IconSymbol::TriangleAlert, IconTone::Warning, MENU_ICON_SIZE),
        readable_text(message).size(13).width(Length::Fill),
    ]
    .spacing(8)
    .align_items(Alignment::Center);

    container(content)
        .padding([10, 12])
        .width(Length::Fixed(ERROR_NOTIFICATION_FLOAT_WIDTH))
        .style(error_notification_style)
        .into()
}

fn transfer_conflict_panel(state: &TransferConflictState) -> Element<'_, Message> {
    let Some(conflict) = state.current_conflict() else {
        return container(readable_text("No pending conflicts").size(14))
            .padding(14)
            .width(Length::Fixed(TRANSFER_CONFLICT_PANEL_WIDTH))
            .style(context_menu_style)
            .into();
    };

    let title = row![
        readable_text("Copy/Move Conflict")
            .size(16)
            .width(Length::Fill),
        readable_text(format!(
            "{} / {}",
            state.current_index + 1,
            state.conflicts.len()
        ))
        .size(12),
    ]
    .spacing(8)
    .align_items(Alignment::Center);

    let apply_label = if state.apply_to_all {
        "On: apply this choice to later compatible conflicts"
    } else {
        "Apply to all: off"
    };

    let mut actions = row![
        conflict_choice_button("Replace", TransferConflictChoice::Replace),
        conflict_choice_button("Skip", TransferConflictChoice::Skip),
        conflict_choice_button("Keep Both", TransferConflictChoice::KeepBoth),
    ]
    .spacing(6)
    .align_items(Alignment::Center);
    if conflict.can_merge() {
        actions = actions.push(conflict_choice_button(
            "Merge Folders",
            TransferConflictChoice::Merge,
        ));
    } else {
        actions = actions.push(
            button(readable_text("Merge Folders").size(12))
                .padding([6, 10])
                .style(context_menu_button_style()),
        );
    }

    let rename = row![
        text_input("New name", &state.rename_input)
            .on_input(Message::TransferConflictRenameInputChanged)
            .on_submit(Message::TransferConflictRenameConfirmed)
            .padding([6, 8])
            .size(14)
            .width(Length::Fill),
        button(readable_text("Rename").size(12))
            .on_press(Message::TransferConflictRenameConfirmed)
            .padding([6, 10])
            .style(context_menu_button_style()),
    ]
    .spacing(6)
    .align_items(Alignment::Center);

    let content = column![
        title,
        readable_text(
            "An item with the same name already exists at the destination. Choose how to continue."
        )
        .size(13),
        transfer_conflict_paths(conflict),
        transfer_conflict_comparison(conflict),
        row![
            button(readable_text(apply_label).size(12))
                .on_press(Message::TransferConflictApplyToAllToggled)
                .padding([6, 10])
                .style(context_menu_button_style()),
            button(readable_text("Cancel").size(12))
                .on_press(Message::TransferConflictCancelRequested)
                .padding([6, 10])
                .style(context_menu_button_style()),
        ]
        .spacing(6),
        actions,
        rename,
    ]
    .spacing(10)
    .width(Length::Fill);

    container(content)
        .padding(14)
        .width(Length::Fixed(TRANSFER_CONFLICT_PANEL_WIDTH))
        .style(context_menu_style)
        .into()
}

fn transfer_conflict_paths(conflict: &TransferConflictItem) -> Element<'_, Message> {
    let source = conflict.source.to_string_lossy();
    let target = conflict.target.to_string_lossy();
    column![
        readable_text(format!(
            "Source: {}",
            format_middle_ellipsized_text(source.as_ref(), TRANSFER_CONFLICT_PATH_MAX_CHARS)
        ))
        .size(12),
        readable_text(format!(
            "Destination: {}",
            format_middle_ellipsized_text(target.as_ref(), TRANSFER_CONFLICT_PATH_MAX_CHARS)
        ))
        .size(12),
    ]
    .spacing(4)
    .into()
}

fn transfer_conflict_comparison(conflict: &TransferConflictItem) -> Element<'_, Message> {
    let source_kind = transfer_metadata_kind(&conflict.source_metadata);
    let target_kind = transfer_metadata_kind(&conflict.target_metadata);
    let size = transfer_size_comparison(&conflict.source_metadata, &conflict.target_metadata);
    let modified = transfer_modified_comparison(
        conflict.source_metadata.modified,
        conflict.target_metadata.modified,
    );

    column![
        readable_text(format!(
            "Type: source {source_kind}, destination {target_kind}"
        ))
        .size(12),
        readable_text(size).size(12),
        readable_text(modified).size(12),
    ]
    .spacing(4)
    .into()
}

fn transfer_metadata_kind(metadata: &TransferConflictMetadata) -> &'static str {
    if metadata.is_directory {
        "Folder"
    } else {
        "File"
    }
}

fn transfer_size_comparison(
    source: &TransferConflictMetadata,
    target: &TransferConflictMetadata,
) -> String {
    let source_size = format_file_size(source.len);
    let target_size = format_file_size(target.len);
    let comparison = match source.len.cmp(&target.len) {
        std::cmp::Ordering::Greater => "source is larger",
        std::cmp::Ordering::Less => "destination is larger",
        std::cmp::Ordering::Equal => "same size",
    };
    format!("Size: source {source_size}, destination {target_size} ({comparison})")
}

fn transfer_modified_comparison(source: Option<SystemTime>, target: Option<SystemTime>) -> String {
    let comparison = match (source, target) {
        (Some(source), Some(target)) if source > target => "source is newer",
        (Some(source), Some(target)) if source < target => "destination is newer",
        (Some(_), Some(_)) => "same modified time",
        _ => "modified time unknown",
    };
    format!("Modified: {comparison}")
}

fn conflict_choice_button(
    label: &'static str,
    choice: TransferConflictChoice,
) -> Button<'static, Message> {
    button(readable_text(label).size(12))
        .on_press(Message::TransferConflictChoiceSelected(choice))
        .padding([6, 10])
        .style(context_menu_button_style())
}

fn action_label(icon: IconSymbol, label: &'static str, size: f32) -> Row<'static, Message> {
    row![themed_icon(icon, IconTone::Normal, size), text(label)]
        .spacing(6)
        .align_items(Alignment::Center)
}

fn sidebar_view(browser: &FileBrowser) -> Element<'_, Message> {
    let sidebar_header = row![
        text("Places").size(16).width(Length::Fill),
        button(themed_icon(
            IconSymbol::Settings,
            IconTone::Normal,
            MENU_ICON_SIZE
        ))
        .on_press(Message::ColumnSettingsToggled)
        .padding([4, 6])
        .style(navigation_icon_button_style()),
    ]
    .spacing(8)
    .align_items(Alignment::Center);

    let mut sidebar = column![sidebar_header].spacing(6).padding(12);

    for location in &browser.sidebar_locations {
        let presentation = sidebar_presentation(browser, location);
        let tone = if presentation.is_selected() {
            IconTone::Selected
        } else {
            IconTone::Normal
        };

        let item_container = container(sidebar_label(IconSymbol::Folder, &location.label, tone))
            .padding([6, 8])
            .width(Length::Fill);
        let item_container = match presentation {
            SidebarPresentation::Selected => item_container.style(selected_sidebar_item_style),
            SidebarPresentation::Hovered => item_container.style(hovered_sidebar_item_style),
            SidebarPresentation::Normal => item_container,
        };

        let item = mouse_area(item_container)
            .on_enter(Message::SidebarHovered(location.path.clone()))
            .on_exit(Message::SidebarHoverCleared(location.path.clone()))
            .on_middle_press(Message::OpenDirectoryInNewTab(location.path.clone()))
            .on_press(Message::NavigateTo(location.path.clone()))
            .interaction(iced::mouse::Interaction::Pointer);

        sidebar = sidebar.push(item);
    }

    let trash_path = trash_location_path();
    let trash_presentation = if browser.is_trash_view {
        SidebarPresentation::Selected
    } else if browser.hovered_sidebar.as_ref() == Some(&trash_path) {
        SidebarPresentation::Hovered
    } else {
        SidebarPresentation::Normal
    };
    let trash_tone = if trash_presentation.is_selected() {
        IconTone::Selected
    } else {
        IconTone::Normal
    };
    let trash_container = container(sidebar_label(
        IconSymbol::Trash,
        TRASH_LOCATION_LABEL,
        trash_tone,
    ))
    .padding([6, 8])
    .width(Length::Fill);
    let trash_container = match trash_presentation {
        SidebarPresentation::Selected => trash_container.style(selected_sidebar_item_style),
        SidebarPresentation::Hovered => trash_container.style(hovered_sidebar_item_style),
        SidebarPresentation::Normal => trash_container,
    };
    let trash_hover_path = trash_path.clone();
    let trash_item = mouse_area(trash_container)
        .on_enter(Message::SidebarHovered(trash_hover_path.clone()))
        .on_exit(Message::SidebarHoverCleared(trash_hover_path))
        .on_press(Message::TrashOpened)
        .on_middle_press(Message::OpenTrashInNewTab)
        .interaction(iced::mouse::Interaction::Pointer);
    sidebar = sidebar.push(trash_item);

    container(scrollable(sidebar).height(Length::Fill))
        .width(Length::Fixed(SIDEBAR_WIDTH))
        .height(Length::Fill)
        .style(sidebar_style)
        .into()
}

fn column_settings_panel(browser: &FileBrowser) -> Element<'_, Message> {
    let mut fixed_count_row = Row::new().spacing(6);
    for count in COLUMN_FIXED_COUNT_OPTIONS {
        fixed_count_row =
            fixed_count_row.push(column_fixed_count_button(count, browser.column_fixed_count));
    }

    container(
        column![
            readable_text("Settings").size(16),
            readable_text("Files").size(13),
            hidden_files_visibility_button(browser),
            readable_text("Column View").size(13),
            row![
                column_view_mode_button(
                    "Unlimited",
                    ColumnViewMode::Unbounded,
                    browser.column_view_mode
                ),
                column_view_mode_button("Fixed", ColumnViewMode::Fixed, browser.column_view_mode),
            ]
            .spacing(6),
            readable_text("Fixed Columns").size(13),
            fixed_count_row,
        ]
        .spacing(6),
    )
    .padding(14)
    .width(Length::Fixed(COLUMN_SETTINGS_FLOAT_WIDTH))
    .style(context_menu_style)
    .into()
}

fn hidden_files_visibility_button(browser: &FileBrowser) -> Button<'static, Message> {
    let status = if browser.options.include_hidden {
        "On"
    } else {
        "Off"
    };
    let label = row![
        readable_text("Show Hidden Files")
            .size(12)
            .width(Length::Fill),
        readable_text(status).size(12),
        switch_control(browser.options.include_hidden),
    ]
    .spacing(8)
    .align_items(Alignment::Center);

    button(container(label).padding([5, 8]).width(Length::Fill))
        .on_press(Message::ShowHiddenFilesToggled)
        .width(Length::Fill)
        .style(context_menu_button_style())
}

fn switch_control(is_on: bool) -> Element<'static, Message> {
    let content = if is_on {
        Row::new()
            .push(Space::with_width(Length::Fill))
            .push(switch_thumb())
    } else {
        Row::new()
            .push(switch_thumb())
            .push(Space::with_width(Length::Fill))
    };

    container(content)
        .padding(3)
        .width(Length::Fixed(38.0))
        .height(Length::Fixed(22.0))
        .style(if is_on {
            switch_track_on_style
        } else {
            switch_track_off_style
        })
        .into()
}

fn switch_thumb() -> Element<'static, Message> {
    container(Space::with_width(Length::Fixed(1.0)))
        .width(Length::Fixed(14.0))
        .height(Length::Fixed(14.0))
        .style(switch_thumb_style)
        .into()
}

fn column_view_mode_button(
    label: &'static str,
    mode: ColumnViewMode,
    selected_mode: ColumnViewMode,
) -> Button<'static, Message> {
    let label = container(readable_text(label).size(12))
        .padding([5, 8])
        .width(Length::Fill);
    let label = if mode == selected_mode {
        label.style(selected_sidebar_item_style)
    } else {
        label
    };

    button(label)
        .on_press(Message::ColumnViewModeSelected(mode))
        .width(Length::Fill)
        .style(context_menu_button_style())
}

fn column_fixed_count_button(count: usize, selected_count: usize) -> Button<'static, Message> {
    let label = container(readable_text(count.to_string()).size(12))
        .padding([5, 8])
        .width(Length::Fill);
    let label = if count == selected_count {
        label.style(selected_sidebar_item_style)
    } else {
        label
    };

    button(label)
        .on_press(Message::ColumnFixedCountSelected(count))
        .width(Length::Fill)
        .style(context_menu_button_style())
}

fn sidebar_presentation(browser: &FileBrowser, location: &SidebarLocation) -> SidebarPresentation {
    if !browser.is_trash_view && location.path == browser.current_dir {
        SidebarPresentation::Selected
    } else if browser.hovered_sidebar.as_ref() == Some(&location.path) {
        SidebarPresentation::Hovered
    } else {
        SidebarPresentation::Normal
    }
}

#[derive(Debug, Clone, Copy)]
enum SidebarPresentation {
    Normal,
    Hovered,
    Selected,
}

impl SidebarPresentation {
    fn is_selected(self) -> bool {
        matches!(self, Self::Selected)
    }
}

fn sidebar_label(icon: IconSymbol, label: &str, tone: IconTone) -> Row<'static, Message> {
    let label = format_middle_ellipsized_text(label, SIDEBAR_LABEL_MAX_CHARS);
    row![
        themed_icon(icon, tone, MENU_ICON_SIZE),
        readable_text(label).width(Length::Fill)
    ]
    .spacing(8)
    .align_items(Alignment::Center)
}

fn context_menu_panel(menu: &ContextMenuState, is_trash_view: bool) -> Element<'_, Message> {
    if is_trash_view {
        return trash_context_menu_panel(menu);
    }

    let paste_button = menu_button(IconSymbol::Copy, "Paste", Message::PastePending);

    let mut menu_content = Column::new().spacing(4).padding(8);
    menu_content = menu_content
        .push(menu_button(
            IconSymbol::Folder,
            "New Folder",
            Message::CreateDirectory(menu.paste_directory.clone()),
        ))
        .push(menu_button(
            IconSymbol::File,
            "New File",
            Message::CreateEmptyFile(menu.paste_directory.clone()),
        ));
    if let Some(path) = &menu.target {
        menu_content = menu_content.push(menu_button(
            IconSymbol::Pencil,
            "Rename",
            Message::BeginRename(path.clone()),
        ));
        if menu.target_is_directory {
            menu_content = menu_content.push(menu_button(
                IconSymbol::Folder,
                "Open in New Tab",
                Message::OpenDirectoryInNewTab(path.clone()),
            ));
        }
        menu_content = menu_content
            .push(menu_button(IconSymbol::Copy, "Copy", Message::CopySelected))
            .push(menu_button(
                IconSymbol::ArrowRight,
                "Move",
                Message::MoveSelected,
            ));
    }
    menu_content = menu_content.push(paste_button.width(Length::Fill));
    if menu.target.is_some() {
        menu_content = menu_content.push(menu_button(
            IconSymbol::Trash,
            "Move to Trash",
            Message::TrashSelected,
        ));
    }

    container(menu_content)
        .width(Length::Fixed(190.0))
        .style(context_menu_style)
        .into()
}

fn trash_context_menu_panel(menu: &ContextMenuState) -> Element<'_, Message> {
    let mut menu_content = Column::new().spacing(4).padding(8);
    if menu.target.is_some() {
        menu_content = menu_content
            .push(menu_button(
                IconSymbol::ArrowLeft,
                "Restore",
                Message::RestoreSelected,
            ))
            .push(menu_button(
                IconSymbol::Trash,
                "Delete Permanently",
                Message::TrashSelected,
            ));
    }
    menu_content = menu_content.push(menu_button(
        IconSymbol::Trash,
        "Empty Trash",
        Message::EmptyTrashRequested,
    ));

    container(menu_content)
        .width(Length::Fixed(190.0))
        .style(context_menu_style)
        .into()
}

fn menu_button(
    icon: IconSymbol,
    label: &'static str,
    message: Message,
) -> Button<'static, Message> {
    button(menu_label(icon, label))
        .on_press(message)
        .width(Length::Fill)
        .style(context_menu_button_style())
}

fn menu_label(icon: IconSymbol, label: &'static str) -> Row<'static, Message> {
    action_label(icon, label, MENU_ICON_SIZE).width(Length::Fill)
}

fn preview_panel(preview: &PreviewState, size: PreviewSize) -> Element<'_, Message> {
    let scroll_height = preview_scroll_height(size);
    let panel = match preview {
        PreviewState::Loading(path) => column![
            readable_text(preview_title(path)).size(14),
            readable_text("Loading preview...").size(14),
        ],
        PreviewState::Ready(PreviewContent::Directory {
            path,
            entries,
            total,
            skipped,
        }) => directory_preview_panel(path, entries, *total, *skipped, scroll_height),
        PreviewState::Ready(PreviewContent::Text {
            path,
            content,
            truncated,
        }) => text_preview_panel(path, content, *truncated, scroll_height),
        PreviewState::Ready(PreviewContent::Archive {
            path,
            entries,
            total,
            truncated,
        }) => archive_preview_panel(path, entries, *total, *truncated, scroll_height),
        PreviewState::Ready(PreviewContent::Image {
            path,
            handle,
            width,
            height,
            ..
        }) => image_preview_panel(path, handle, *width, *height, scroll_height),
        PreviewState::Error(error) => column![
            readable_text("Preview").size(14),
            readable_text(error).size(14),
        ],
    }
    .spacing(6);

    let content = panel.height(Length::Fill);

    container(content)
        .padding(14)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(preview_panel_style)
        .into()
}

fn preview_scroll_height(size: PreviewSize) -> f32 {
    (size.height - PREVIEW_HEADER_RESERVED_HEIGHT).max(PREVIEW_MIN_SCROLL_HEIGHT)
}

fn directory_preview_panel(
    path: &std::path::PathBuf,
    entries: &[crate::model::PreviewEntry],
    total: usize,
    skipped: usize,
    scroll_height: f32,
) -> Column<'static, Message> {
    let mut listing = Column::new().spacing(3);
    if entries.is_empty() {
        listing = listing.push(text("Empty directory").size(14));
    } else {
        for entry in entries {
            listing = listing.push(preview_entry_row(entry));
        }
    }

    let summary = if total > entries.len() {
        format!("Showing {} of {} items", entries.len(), total)
    } else {
        format!("{} items", total)
    };
    let summary = if skipped > 0 {
        format!("{summary}, {skipped} skipped")
    } else {
        summary
    };

    column![
        readable_text(preview_title(path)).size(14),
        text(summary).size(14),
        scrollable(listing)
            .direction(preview_scroll_direction())
            .height(Length::Fixed(scroll_height)),
    ]
}

fn archive_preview_panel(
    path: &std::path::PathBuf,
    entries: &[PreviewArchiveEntry],
    total: usize,
    truncated: bool,
    scroll_height: f32,
) -> Column<'static, Message> {
    let mut listing = Column::new().spacing(3);
    if entries.is_empty() {
        listing = listing.push(readable_text("Empty archive").size(14));
    } else {
        for entry in visible_archive_entries(entries) {
            listing = listing.push(preview_archive_entry_row(entry));
        }
    }

    column![
        readable_text(preview_title(path)).size(14),
        readable_text(archive_preview_summary(entries.len(), total, truncated)).size(14),
        scrollable(listing)
            .direction(preview_scroll_direction())
            .height(Length::Fixed(scroll_height)),
    ]
}

fn archive_preview_summary(loaded_count: usize, total: usize, truncated: bool) -> String {
    if truncated {
        format!("Showing first {loaded_count} of {total} entries")
    } else {
        format!("{total} entries")
    }
}

fn visible_archive_entries(entries: &[PreviewArchiveEntry]) -> Vec<&PreviewArchiveEntry> {
    entries
        .iter()
        .filter(|entry| archive_entry_visible(entry, entries))
        .collect()
}

fn archive_entry_visible(entry: &PreviewArchiveEntry, entries: &[PreviewArchiveEntry]) -> bool {
    let mut parent = entry.parent;
    while let Some(parent_id) = parent {
        let Some(parent_entry) = entries.get(parent_id) else {
            return false;
        };
        if !parent_entry.is_expanded {
            return false;
        }
        parent = parent_entry.parent;
    }

    true
}

fn preview_archive_entry_row(entry: &PreviewArchiveEntry) -> Element<'static, Message> {
    let name = format_middle_ellipsized_text(&entry.name, PREVIEW_ENTRY_NAME_MAX_CHARS);
    let indent = Space::with_width(Length::Fixed(
        entry.depth as f32 * PREVIEW_ARCHIVE_INDENT_WIDTH,
    ));
    let toggle: Element<'static, Message> = if entry.is_directory() {
        text(if entry.is_expanded { "v" } else { ">" })
            .size(13)
            .width(Length::Fixed(PREVIEW_ARCHIVE_TOGGLE_WIDTH))
            .into()
    } else {
        Space::with_width(Length::Fixed(PREVIEW_ARCHIVE_TOGGLE_WIDTH)).into()
    };
    let row_content = row![
        indent,
        toggle,
        themed_icon(
            preview_entry_icon_symbol(entry.kind, &entry.name),
            IconTone::Normal,
            PREVIEW_ICON_SIZE,
        ),
        readable_text(name).size(14).width(Length::Fill),
    ]
    .spacing(6)
    .align_items(Alignment::Center);
    let row_container = container(row_content).padding([3, 6]).width(Length::Fill);

    if entry.is_directory() {
        mouse_area(row_container)
            .on_press(Message::ArchiveDirectoryToggled(entry.id))
            .interaction(iced::mouse::Interaction::Pointer)
            .into()
    } else {
        row_container.into()
    }
}

fn text_preview_panel(
    path: &std::path::PathBuf,
    content: &str,
    truncated: bool,
    scroll_height: f32,
) -> Column<'static, Message> {
    let preview_text = numbered_preview_text(content);
    let status = if truncated {
        format!(
            "Showing first {}",
            format_file_size(PREVIEW_TEXT_LIMIT as u64)
        )
    } else {
        "Complete text preview".to_owned()
    };

    column![
        readable_text(preview_title(path)).size(14),
        text(status).size(14),
        scrollable(readable_text(preview_text).size(14))
            .direction(preview_scroll_direction())
            .height(Length::Fixed(scroll_height)),
    ]
}

fn image_preview_panel(
    path: &std::path::PathBuf,
    handle: &image::Handle,
    width: u32,
    height: u32,
    scroll_height: f32,
) -> Column<'static, Message> {
    column![
        readable_text(preview_title(path)).size(14),
        text(format!("Image preview · {width} × {height}")).size(14),
        container(
            image::Image::new(handle.clone())
                .width(Length::Fill)
                .height(Length::Fixed(scroll_height)),
        )
        .width(Length::Fill)
        .height(Length::Fixed(scroll_height)),
    ]
}

fn numbered_preview_text(content: &str) -> String {
    if content.is_empty() {
        return "1 | (empty file)".to_owned();
    }

    let line_count = content.lines().count().max(1);
    let width = line_count.to_string().len();
    let mut numbered = String::new();
    for (index, line) in content.lines().enumerate() {
        numbered.push_str(&format!(
            "{:>width$} | {}\n",
            index + 1,
            line,
            width = width
        ));
    }
    numbered
}

fn preview_title(path: &Path) -> String {
    let path = path.to_string_lossy();
    let path = format_middle_ellipsized_text(path.as_ref(), PREVIEW_PATH_MAX_CHARS);
    format!("Preview: {path}")
}

fn preview_scroll_direction() -> iced::widget::scrollable::Direction {
    iced::widget::scrollable::Direction::Vertical(
        iced::widget::scrollable::Properties::new()
            .width(6.0)
            .scroller_width(6.0),
    )
}

fn preview_entry_row(entry: &crate::model::PreviewEntry) -> Row<'static, Message> {
    let name = format_middle_ellipsized_text(&entry.name, PREVIEW_ENTRY_NAME_MAX_CHARS);

    row![
        themed_icon(
            preview_entry_icon_symbol(entry.kind, &entry.name),
            IconTone::Normal,
            PREVIEW_ICON_SIZE
        ),
        readable_text(name).size(14),
    ]
    .spacing(8)
    .align_items(Alignment::Center)
}

fn themed_icon(symbol: IconSymbol, tone: IconTone, size: f32) -> Svg<Theme> {
    symbol.view(size).style(icon_tone_style(tone))
}

fn icon_tone_style(tone: IconTone) -> iced::theme::Svg {
    match tone {
        IconTone::Normal => icon_svg_style(),
        IconTone::Selected => selected_icon_svg_style(),
        IconTone::Warning => warning_icon_svg_style(),
    }
}

#[derive(Debug, Clone, Copy)]
enum IconTone {
    Normal,
    Selected,
    Warning,
}
