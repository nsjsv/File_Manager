mod archive_creation;
mod archive_extraction;
mod auxiliary_window_layout;
mod batch_rename;
mod file_operation_verification_settings;
mod floating_panels;
mod markdown_preview;
mod network_connections;
mod network_settings;
mod option_controls;
mod preview_panel;
mod properties_window;
mod rendering_settings;
mod search_index_settings;
mod search_mode_prompt;
mod search_panel;
mod settings_window;
mod shortcut_settings;
mod sidebar_panel;
mod startup_index_setup;
mod tab_motion;
mod text_preview_panel;
mod toggle_switch;
mod toolbar_controls;
mod transfer_conflict;

pub(crate) use preview_panel::view_preview_window;
pub(crate) use properties_window::view_properties_window;
pub(crate) use search_panel::{
    search_input_id, search_results_id, view_search_window, SEARCH_RESULTS_HEIGHT,
    SEARCH_RESULTS_PADDING, SEARCH_RESULT_ROW_HEIGHT, SEARCH_RESULT_ROW_SPACING,
};
pub(crate) use settings_window::view_settings_window;
pub(crate) use tab_motion::translated_with_width_overflow;

use std::path::Path;

use file_core::{DirectoryEntry, FileKind};
use iced::widget::{
    button, container, mouse_area, opaque, row, stack, text_input, Column, Row, Space, Svg,
};
use iced::{Alignment, Element, Length, Point, Theme};

use crate::anchored_popup::anchored_popup;
use crate::app::panes::BrowserPaneView;
use crate::app::smooth_scroll::smooth_scroll_id;
use crate::app::FileBrowser;
use crate::appearance::{
    app_content_style, drag_preview_style, icon_svg_style, navigation_icon_button_style,
    path_suggestion_item_style, path_suggestions_style, selected_icon_svg_style,
    selected_path_suggestion_item_style, selected_tab_item_style, tab_item_style,
    tab_split_overlay_style, tab_strip_style, warning_icon_svg_style,
};
use crate::floating_surface::{
    dismissable_blocking_floating_surface, floating_surface, modal_floating_surface,
    pass_through_dismissable_floating_surface, FloatingContent, FloatingPlacement,
};
use crate::formatting::format_middle_ellipsized_text;
use crate::icons::{file_entry_icon_symbol, IconSymbol};
use crate::list_view::list_browser_view;
use crate::model::{
    BrowserPaneId, BrowserPaneLayout, BrowserViewMode, Message, OperationQueuePanelMode,
    ScrollbarRegion, SplitAxis, TRASH_LOCATION_LABEL,
};
use crate::operation_queue_view::{
    operation_queue_indicator, operation_queue_panel, OPERATION_QUEUE_INDICATOR_BOTTOM,
    OPERATION_QUEUE_INDICATOR_RIGHT, OPERATION_QUEUE_PANEL_BOTTOM,
};
use crate::selection_marquee::selection_marquee_overlay;
use crate::three_column_view::column_browser_view;
use crate::typography::readable_text;

use self::network_connections::network_connection_editor_panel;
use archive_creation::archive_creation_panel;
use archive_extraction::archive_extraction_panel;
use batch_rename::batch_rename_panel;
use floating_panels::{
    context_menu_panel, destructive_action_confirmation_panel, error_notification_panel,
    file_drop_operation_panel, open_with_panel,
};
use rendering_settings::renderer_restart_notice_panel;
use search_mode_prompt::search_mode_prompt_panel;
use sidebar_panel::sidebar_view;
use startup_index_setup::startup_index_setup_panel;
use toolbar_controls::{navigation_button_group, view_mode_button_group};
use transfer_conflict::transfer_conflict_panel;

const TOOLBAR_ICON_SIZE: f32 = 16.0;
const VIEW_MODE_ICON_SIZE: f32 = 16.0;
const TAB_ICON_SIZE: f32 = 14.0;
const TAB_CLOSE_ICON_SIZE: f32 = 12.0;
const TAB_CLOSE_SLOT_WIDTH: f32 = 22.0;
const TAB_BAR_EXPANDED_HEIGHT: f32 = 34.0;
const TAB_FILL_PORTION: u16 = 1000;
const TAB_DRAG_PREVIEW_WIDTH: f32 = 220.0;
pub(super) const MENU_ICON_SIZE: f32 = 16.0;
const TAB_LABEL_MAX_CHARS: usize = 24;
const PATH_SUGGESTION_MAX_CHARS: usize = 72;
const ERROR_NOTIFICATION_CONTENT_OFFSET_X: f32 = 18.0;
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

pub(crate) fn batch_rename_preview_name_input_id(path: &Path) -> iced::widget::Id {
    iced::widget::Id::from(format!("batch-rename-preview-name-{}", path.display()))
}

pub(crate) fn path_input_id(pane_id: BrowserPaneId) -> iced::widget::Id {
    iced::widget::Id::from(format!("path-input-{}", pane_id.key()))
}

pub(crate) fn column_browser_scroll_id(pane_id: BrowserPaneId) -> iced::widget::Id {
    smooth_scroll_id(&ScrollbarRegion::ColumnBrowser(pane_id))
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
    let onboarding_active =
        browser.search_mode_prompt.is_some() || browser.startup_index_setup.is_some();
    let content: Element<'_, Message> = if onboarding_active {
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(app_content_style)
            .into()
    } else {
        let pane_layer: Element<'_, Message> = container(
            row![
                Space::new().width(Length::Fixed(browser.sidebar_width)),
                container(panes_view(browser))
                    .width(Length::Fill)
                    .height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(app_content_style)
        .into();
        stack([pane_layer, opaque(sidebar_view(browser))])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    };

    let mut floating = Vec::new();
    let mut floating_input = BrowserFloatingInput::Plain;
    if let Some(prompt) = &browser.search_mode_prompt {
        floating_input = BrowserFloatingInput::Modal;
        floating.push(FloatingContent {
            element: search_mode_prompt_panel(prompt),
            placement: FloatingPlacement::Center,
        });
    } else if let Some(startup_index_setup) = &browser.startup_index_setup {
        floating_input = BrowserFloatingInput::Modal;
        floating.push(FloatingContent {
            element: startup_index_setup_panel(
                startup_index_setup,
                browser.scrollbar_visibility_for(&ScrollbarRegion::StartupIndexSetup),
            ),
            placement: FloatingPlacement::Center,
        });
    } else if let Some(confirmation) = &browser.destructive_action_confirmation {
        floating_input = BrowserFloatingInput::Modal;
        floating.push(FloatingContent {
            element: destructive_action_confirmation_panel(confirmation),
            placement: FloatingPlacement::Center,
        });
    } else if let Some(file_drop_prompt) = &browser.file_drop_prompt {
        floating_input = BrowserFloatingInput::Modal;
        floating.push(FloatingContent {
            element: file_drop_operation_panel(file_drop_prompt),
            placement: FloatingPlacement::Center,
        });
    } else if let Some(conflict) = &browser.transfer_conflict {
        floating_input = BrowserFloatingInput::Modal;
        floating.push(FloatingContent {
            element: transfer_conflict_panel(conflict, &browser.thumbnail_cache),
            placement: FloatingPlacement::Center,
        });
    } else if let Some(archive_extraction) = &browser.archive_extraction {
        floating_input = BrowserFloatingInput::Modal;
        floating.push(FloatingContent {
            element: archive_extraction_panel(archive_extraction),
            placement: FloatingPlacement::Center,
        });
    } else if let Some(batch_rename) = &browser.batch_rename {
        floating_input = BrowserFloatingInput::Modal;
        floating.push(FloatingContent {
            element: batch_rename_panel(
                batch_rename,
                browser.scrollbar_visibility_for(&ScrollbarRegion::BatchRenamePreview),
            ),
            placement: FloatingPlacement::Center,
        });
    } else if let Some(editor) = &browser.network_connection_editor {
        floating_input = BrowserFloatingInput::Modal;
        floating.push(FloatingContent {
            element: network_connection_editor_panel(editor),
            placement: FloatingPlacement::Center,
        });
    } else if let Some(marquee) = &browser.selection_marquee {
        floating.push(selection_marquee_overlay(marquee));
    } else if let Some(drag_preview) = drag_preview_panel(browser) {
        floating.push(FloatingContent {
            element: drag_preview,
            placement: FloatingPlacement::Free(drag_preview_position(browser.cursor_position)),
        });
    } else if let Some(archive_creation) = &browser.archive_creation {
        floating_input = BrowserFloatingInput::DismissibleBlocking;
        floating.push(FloatingContent {
            element: archive_creation_panel(archive_creation),
            placement: FloatingPlacement::Center,
        });
    } else if let Some(context_menu) = &browser.context_menu {
        floating_input = BrowserFloatingInput::DismissibleBlocking;
        floating.push(FloatingContent {
            element: context_menu_panel(
                context_menu,
                browser.is_trash_view,
                browser.active_pane_id(),
                &browser.user_config().list_view_preferences,
            ),
            placement: FloatingPlacement::At(context_menu.position()),
        });
    } else if let Some(open_with) = &browser.open_with {
        floating_input = BrowserFloatingInput::DismissibleBlocking;
        floating.push(FloatingContent {
            element: open_with_panel(
                open_with,
                browser.scrollbar_visibility_for(&ScrollbarRegion::OpenWithApplications),
            ),
            placement: FloatingPlacement::Center,
        });
    }

    if let Some(bounds) = browser.tab_split_overlay_bounds() {
        floating.push(FloatingContent {
            element: tab_split_overlay(bounds.width, bounds.height),
            placement: FloatingPlacement::Free(bounds.top_left),
        });
    }

    if let Some(bounds) = browser.pane_drag_overlay_bounds() {
        floating.push(FloatingContent {
            element: tab_split_overlay(bounds.width, bounds.height),
            placement: FloatingPlacement::Free(bounds.top_left),
        });
    }

    if let Some(tab_preview) = tab_drag_preview_panel(browser) {
        floating.push(FloatingContent {
            element: tab_preview,
            placement: FloatingPlacement::Free(drag_preview_position(browser.cursor_position)),
        });
    }

    if let Some(pane_preview) = pane_drag_preview_panel(browser) {
        floating.push(FloatingContent {
            element: pane_preview,
            placement: FloatingPlacement::Free(drag_preview_position(browser.cursor_position)),
        });
    }

    if let Some(error) = browser.error.as_deref() {
        floating.push(FloatingContent {
            element: error_notification_panel(error),
            placement: FloatingPlacement::At(iced::Point::new(
                browser.sidebar_width + ERROR_NOTIFICATION_CONTENT_OFFSET_X,
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
                browser.sidebar_width + ERROR_NOTIFICATION_CONTENT_OFFSET_X,
                notice_y,
            )),
        });
    }

    if !onboarding_active && browser.operation_queue.is_panel_open() {
        let queue_dismissal = match browser.operation_queue_panel_mode {
            OperationQueuePanelMode::PassivePreview => BrowserFloatingInput::DismissiblePassThrough,
            OperationQueuePanelMode::InteractiveList => BrowserFloatingInput::DismissibleBlocking,
        };
        floating_input = floating_input.with_additional_panel(queue_dismissal);
        floating.push(FloatingContent {
            element: operation_queue_panel(
                &browser.operation_queue,
                browser.scrollbar_visibility_for(&ScrollbarRegion::OperationQueue),
            ),
            placement: FloatingPlacement::BottomLeft {
                left: browser.sidebar_width + 12.0,
                bottom: OPERATION_QUEUE_PANEL_BOTTOM,
            },
        });
    }

    if !onboarding_active {
        if let Some(indicator) = operation_queue_indicator(&browser.operation_queue) {
            floating.push(FloatingContent {
                element: indicator,
                placement: FloatingPlacement::BottomRightInArea {
                    area_width: browser.sidebar_width,
                    right: OPERATION_QUEUE_INDICATOR_RIGHT,
                    bottom: OPERATION_QUEUE_INDICATOR_BOTTOM,
                },
            });
        }
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

fn panes_view(browser: &FileBrowser) -> Element<'_, Message> {
    match browser.pane_layout {
        BrowserPaneLayout::Single { active } => pane_view(browser, active),
        BrowserPaneLayout::Split {
            axis,
            first,
            second,
            ..
        } => match axis {
            SplitAxis::Horizontal => Row::new()
                .spacing(0)
                .push(pane_view(browser, first))
                .push(pane_view(browser, second))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            SplitAxis::Vertical => Column::new()
                .spacing(0)
                .push(pane_view(browser, first))
                .push(pane_view(browser, second))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        },
    }
}

fn pane_view(browser: &FileBrowser, pane_id: BrowserPaneId) -> Element<'_, Message> {
    let Some(pane) = browser.pane_view(pane_id) else {
        return Space::new().width(Length::Fill).height(Length::Fill).into();
    };

    let navigation_bar = row![
        navigation_button_group(pane_id),
        path_input_panel(pane),
        view_mode_button_group(pane),
    ]
    .spacing(8)
    .align_y(Alignment::Start);

    let header_content = container(navigation_bar).padding(18).width(Length::Fill);
    let mut main_content = Column::new().spacing(0).push(header_content);
    if pane.tab_bar_should_occupy_layout() {
        main_content = main_content.push(tab_bar(pane));
    }

    let pane_content = main_content
        .push(browser_content_view(browser, pane))
        .width(Length::Fill)
        .height(Length::Fill);

    mouse_area(pane_content)
        .on_enter(Message::PaneCursorEntered(pane_id))
        .on_exit(Message::PaneCursorExited(pane_id))
        .into()
}

fn browser_content_view<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
) -> Element<'a, Message> {
    match pane.view_mode {
        BrowserViewMode::Columns => column_browser_view(browser, pane),
        BrowserViewMode::List => list_browser_view(browser, pane),
    }
}

fn tab_split_overlay(width: f32, height: f32) -> Element<'static, Message> {
    container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .style(tab_split_overlay_style)
        .into()
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

fn tab_drag_preview_panel(browser: &FileBrowser) -> Option<Element<'_, Message>> {
    let preview = browser.tab_drag_preview()?;
    Some(
        container(tab_title_content(
            preview.directory,
            preview.is_trash_view,
            IconTone::Selected,
        ))
        .padding([7, 10])
        .width(Length::Fixed(TAB_DRAG_PREVIEW_WIDTH))
        .style(selected_tab_item_style)
        .into(),
    )
}

fn pane_drag_preview_panel(browser: &FileBrowser) -> Option<Element<'_, Message>> {
    let preview = browser.pane_drag_preview()?;
    Some(
        container(tab_title_content(
            preview.directory,
            preview.is_trash_view,
            IconTone::Selected,
        ))
        .padding([7, 10])
        .width(Length::Fixed(TAB_DRAG_PREVIEW_WIDTH))
        .style(selected_tab_item_style)
        .into(),
    )
}

fn tab_bar<'a>(pane: BrowserPaneView<'a>) -> Element<'a, Message> {
    let reveal_fraction = pane.tab_bar_reveal_fraction;
    if reveal_fraction <= f32::EPSILON && pane.tabs.len() <= 1 {
        return Space::new().height(Length::Fixed(0.0)).into();
    }

    let mut tabs = Row::new()
        .spacing(6)
        .align_y(Alignment::Center)
        .width(Length::Fill);
    for tab in pane.tabs {
        tabs = tabs.push(tab_button(
            pane.id,
            tab.id,
            tab.directory.as_path(),
            tab.is_trash_view,
            tab.id == pane.active_tab_id,
            pane.tab_width_fraction(tab.id),
            pane.tab_shift_offset(tab.id),
        ));
    }

    container(tabs)
        .height(Length::Fixed(TAB_BAR_EXPANDED_HEIGHT * reveal_fraction))
        .width(Length::Fill)
        .padding([3, 18])
        .style(tab_strip_style)
        .into()
}

fn tab_button<'a>(
    pane_id: BrowserPaneId,
    tab_id: usize,
    directory: &'a Path,
    is_trash_view: bool,
    is_active: bool,
    width_fraction: f32,
    shift_offset: f32,
) -> Element<'a, Message> {
    let tone = if is_active {
        IconTone::Selected
    } else {
        IconTone::Normal
    };
    let title = tab_title_content(directory, is_trash_view, tone);
    let label = row![
        Space::new().width(Length::Fixed(TAB_CLOSE_SLOT_WIDTH)),
        container(title).center_x(Length::Fill),
        button(themed_icon(IconSymbol::Close, tone, TAB_CLOSE_ICON_SIZE))
            .on_press(Message::TabCloseRequested(pane_id, tab_id))
            .padding([2, 2])
            .width(Length::Fixed(TAB_CLOSE_SLOT_WIDTH))
            .style(navigation_icon_button_style()),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let tab = container(label)
        .padding([4, 8])
        .width(Length::Fill)
        .clip(true);
    let tab = tab.style(if is_active {
        selected_tab_item_style
    } else {
        tab_item_style
    });

    let tab = tab_motion::translated(tab, shift_offset, 0.0);

    container(
        mouse_area(tab)
            .on_press(Message::TabPressed(pane_id, tab_id))
            .on_middle_press(Message::TabCloseRequested(pane_id, tab_id))
            .on_enter(Message::TabDragEntered(pane_id, tab_id))
            .on_release(Message::TabDragFinished)
            .interaction(iced::mouse::Interaction::Pointer),
    )
    .width(Length::FillPortion(tab_width_portion(width_fraction)))
    .into()
}

fn tab_title_content<'a>(
    directory: &'a Path,
    is_trash_view: bool,
    tone: IconTone,
) -> Row<'a, Message> {
    let symbol = if is_trash_view {
        IconSymbol::Trash
    } else {
        IconSymbol::Folder
    };
    row![
        themed_icon(symbol, tone, TAB_ICON_SIZE),
        readable_text(tab_title_text(directory, is_trash_view)).size(13),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
}

fn tab_width_portion(intro_fraction: f32) -> u16 {
    ((TAB_FILL_PORTION as f32) * intro_fraction.clamp(0.0, 1.0))
        .round()
        .max(1.0) as u16
}

fn tab_title_text(directory: &Path, is_trash_view: bool) -> String {
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

fn path_input_panel<'a>(pane: BrowserPaneView<'a>) -> Element<'a, Message> {
    if pane.is_trash_view {
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

    let input = text_input(
        &crate::localization::translate_current("Path"),
        pane.path_input,
    )
    .id(path_input_id(pane.id))
    .on_input(move |value| Message::PathInputChanged(pane.id, value))
    .on_submit(Message::PathInputSubmitted(pane.id))
    .padding([7, 10])
    .size(16)
    .width(Length::Fill);

    let popup = (!pane.path_suggestions.is_empty()).then(|| path_suggestions_panel(pane));

    container(anchored_popup(input, popup))
        .width(Length::Fill)
        .into()
}

fn path_suggestions_panel<'a>(pane: BrowserPaneView<'a>) -> Element<'a, Message> {
    let mut suggestions = Column::new().spacing(3).padding(4);
    for (index, suggestion) in pane.path_suggestions.iter().enumerate() {
        suggestions = suggestions.push(path_suggestion_row(
            pane.id,
            suggestion,
            pane.path_suggestion_selection == Some(index),
        ));
    }

    container(suggestions)
        .width(Length::Fill)
        .style(path_suggestions_style)
        .into()
}

fn path_suggestion_row(
    pane_id: BrowserPaneId,
    path: &Path,
    is_selected: bool,
) -> Element<'_, Message> {
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
        .on_press(Message::PathSuggestionSelected(pane_id, path.to_path_buf()))
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
