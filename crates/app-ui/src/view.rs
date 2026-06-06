mod floating_panels;
mod markdown_preview;
mod preview_panel;
mod rendering_settings;
mod search_panel;
mod toggle_switch;

pub(crate) use preview_panel::view_preview_window;
pub(crate) use search_panel::{
    search_input_id, search_results_id, view_search_window, SEARCH_RESULTS_HEIGHT,
    SEARCH_RESULTS_PADDING, SEARCH_RESULT_ROW_HEIGHT, SEARCH_RESULT_ROW_SPACING,
};

use std::path::Path;

use file_core::{DirectoryEntry, FileKind};
use iced::widget::{
    button, column, container, mouse_area, row, scrollable, text_input, Button, Column, Row, Svg,
};
use iced::{Alignment, Element, Length, Point, Theme};

use crate::anchored_popup::anchored_popup;
use crate::app::FileBrowser;
use crate::appearance::{
    app_content_style, auto_hide_horizontal_scrollbar_direction, auto_hide_scrollbar_style,
    drag_preview_style, icon_svg_style, navigation_icon_button_style, path_suggestion_item_style,
    path_suggestions_style, selected_icon_svg_style, selected_path_suggestion_item_style,
    selected_sidebar_item_style, warning_icon_svg_style,
};
use crate::floating_surface::{
    dismissable_blocking_floating_surface, floating_surface, modal_floating_surface,
    pass_through_dismissable_floating_surface, FloatingContent, FloatingPlacement,
};
use crate::formatting::format_middle_ellipsized_text;
use crate::icons::{file_entry_icon_symbol, IconSymbol};
use crate::model::{Message, OperationQueuePanelMode, TRASH_LOCATION_LABEL};
use crate::operation_queue_view::{
    operation_queue_indicator, operation_queue_panel, OPERATION_QUEUE_INDICATOR_BOTTOM,
    OPERATION_QUEUE_INDICATOR_RIGHT, OPERATION_QUEUE_PANEL_BOTTOM,
};
use crate::selection_marquee::selection_marquee_overlay;
use crate::sidebar::SIDEBAR_WIDTH;
use crate::three_column_view::column_browser_view;
use crate::typography::readable_text;

use floating_panels::{
    column_settings_panel, context_menu_panel, destructive_action_confirmation_panel,
    error_notification_panel, sidebar_view, transfer_conflict_panel,
};
use rendering_settings::renderer_restart_notice_panel;

const TOOLBAR_ICON_SIZE: f32 = 16.0;
const TAB_ICON_SIZE: f32 = 14.0;
const TAB_CLOSE_ICON_SIZE: f32 = 12.0;
pub(super) const MENU_ICON_SIZE: f32 = 16.0;
const TAB_LABEL_MAX_CHARS: usize = 24;
const PATH_SUGGESTION_MAX_CHARS: usize = 72;
const ERROR_NOTIFICATION_FLOAT_X: f32 = SIDEBAR_WIDTH + 18.0;
const ERROR_NOTIFICATION_FLOAT_Y: f32 = 18.0;
const RENDERER_RESTART_NOTICE_ERROR_OFFSET_Y: f32 = 58.0;
const DRAG_PREVIEW_ICON_SIZE: f32 = 18.0;
const DRAG_PREVIEW_LABEL_MAX_CHARS: usize = 34;
const DRAG_PREVIEW_OFFSET_X: f32 = 14.0;
const DRAG_PREVIEW_OFFSET_Y: f32 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserFloatingInput {
    Plain,
    Modal,
    DismissibleBlocking,
    DismissiblePassThrough,
}

impl BrowserFloatingInput {
    fn with_additional_panel(self, next: Self) -> Self {
        match (self, next) {
            (Self::Modal, _) | (_, Self::Modal) => Self::Modal,
            (Self::DismissibleBlocking, _) | (_, Self::DismissibleBlocking) => {
                Self::DismissibleBlocking
            }
            (Self::DismissiblePassThrough, _) | (_, Self::DismissiblePassThrough) => {
                Self::DismissiblePassThrough
            }
            (Self::Plain, Self::Plain) => Self::Plain,
        }
    }
}

pub(crate) fn rename_input_id() -> iced::widget::Id {
    iced::widget::Id::new("rename-input")
}

pub(crate) fn path_input_id() -> iced::widget::Id {
    iced::widget::Id::new("path-input")
}

pub(crate) fn column_browser_scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("column-browser")
}

pub(super) fn auxiliary_window_message(message: &'static str) -> Element<'static, Message> {
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
    .align_y(Alignment::Start);

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
    let mut floating_input = BrowserFloatingInput::Plain;
    if let Some(confirmation) = &browser.destructive_action_confirmation {
        floating_input = BrowserFloatingInput::Modal;
        floating.push(FloatingContent {
            element: destructive_action_confirmation_panel(confirmation),
            placement: FloatingPlacement::Center,
        });
    } else if let Some(conflict) = &browser.transfer_conflict {
        floating_input = BrowserFloatingInput::Modal;
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
        floating_input = BrowserFloatingInput::DismissibleBlocking;
        floating.push(FloatingContent {
            element: context_menu_panel(context_menu, browser.is_trash_view),
            placement: FloatingPlacement::At(context_menu.position()),
        });
    } else if browser.is_column_view_settings_open {
        floating_input = BrowserFloatingInput::DismissibleBlocking;
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

    if browser.renderer_restart_notice_visible {
        let notice_y = if browser.error.is_some() {
            ERROR_NOTIFICATION_FLOAT_Y + RENDERER_RESTART_NOTICE_ERROR_OFFSET_Y
        } else {
            ERROR_NOTIFICATION_FLOAT_Y
        };
        floating.push(FloatingContent {
            element: renderer_restart_notice_panel(),
            placement: FloatingPlacement::At(iced::Point::new(
                ERROR_NOTIFICATION_FLOAT_X,
                notice_y,
            )),
        });
    }

    if browser.operation_queue.is_panel_open() {
        let queue_dismissal = match browser.operation_queue_panel_mode {
            OperationQueuePanelMode::PassivePreview => BrowserFloatingInput::DismissiblePassThrough,
            OperationQueuePanelMode::InteractiveList => BrowserFloatingInput::DismissibleBlocking,
        };
        floating_input = floating_input.with_additional_panel(queue_dismissal);
        floating.push(FloatingContent {
            element: operation_queue_panel(&browser.operation_queue, browser.scrollbar_visibility),
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

    match floating_input {
        BrowserFloatingInput::Plain => floating_surface(content, floating),
        BrowserFloatingInput::Modal => modal_floating_surface(content, floating),
        BrowserFloatingInput::DismissibleBlocking => {
            dismissable_blocking_floating_surface(content, floating, Message::DismissFloating)
        }
        BrowserFloatingInput::DismissiblePassThrough => {
            pass_through_dismissable_floating_surface(content, floating, Message::DismissFloating)
        }
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
    .align_y(Alignment::Center);

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

    let name = path.file_name().unwrap_or(path.as_os_str());
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
    let mut tabs = Row::new().spacing(6).align_y(Alignment::Center);
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
            .direction(auto_hide_horizontal_scrollbar_direction(
                browser.scrollbar_visibility,
                6.0,
            ))
            .style(auto_hide_scrollbar_style(browser.scrollbar_visibility))
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
    .align_y(Alignment::Center);

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
            .align_y(Alignment::Center),
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

fn path_suggestion_row(path: &Path, is_selected: bool) -> Element<'_, Message> {
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
        .on_press(Message::PathSuggestionSelected(path.to_path_buf()))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

pub(super) fn themed_icon(symbol: IconSymbol, tone: IconTone, size: f32) -> Svg<'static, Theme> {
    symbol.view(size).style(icon_tone_style(tone))
}

pub(super) fn icon_tone_style(
    tone: IconTone,
) -> fn(&Theme, iced::widget::svg::Status) -> iced::widget::svg::Style {
    match tone {
        IconTone::Normal => icon_svg_style(),
        IconTone::Selected => selected_icon_svg_style(),
        IconTone::Warning => warning_icon_svg_style(),
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum IconTone {
    Normal,
    Selected,
    Warning,
}
