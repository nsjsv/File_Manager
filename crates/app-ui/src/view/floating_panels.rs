use iced::widget::{
    button, checkbox, column, container, mouse_area, row, scrollable, Button, Column, Row, Space,
};
use iced::{Alignment, Element, Length};

use desktop_linux::FileClipboardOperation;

use crate::app::archive_creation::ArchiveCreationMessage;
use crate::app::checksum::ChecksumMessage;
use crate::app::convert::ConvertMessage;
use crate::app::scrollbar::{enhanced_scrollbar, scrollbar_on_scroll, ScrollbarAxis};
use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use crate::appearance::{
    context_menu_item_button_style, context_menu_style, enhanced_scrollbar_style,
    enhanced_vertical_scrollbar_direction, error_notification_style, navigation_icon_button_style,
};
use crate::formatting::format_middle_ellipsized_text;
use crate::icons::IconSymbol;
use crate::model::{
    BatchRenameMessage, BrowserPaneId, ContextMenuState, DestructiveActionConfirmation,
    FileContextMenuExpansion, FileContextMenuState, FileDropPrompt, FilePropertiesMessage,
    ListColumnConfig, ListColumnKind, ListViewPreferences, Message, ScrollbarRegion,
    ScrollbarViewport, ScrollbarVisibility, SearchContextMenuState, SearchEntryTypePreset,
    SidebarBookmarkContextMenuState,
};
use crate::open_with::OpenWithState;
use crate::sidebar_devices::SidebarDeviceContextMenuState;
use crate::typography::{localized_text, readable_text};

use super::network_connections::network_connection_context_menu_panel;
use super::option_controls::{action_choice_row, primary_action_button, secondary_action_button};
use super::{themed_icon, IconTone, MENU_ICON_SIZE};

const ERROR_NOTIFICATION_FLOAT_WIDTH: f32 = 560.0;
const ERROR_NOTIFICATION_MAX_CHARS: usize = 96;
const DESTRUCTIVE_CONFIRMATION_PANEL_WIDTH: f32 = 460.0;
const FILE_DROP_OPERATION_PANEL_WIDTH: f32 = 460.0;
const FILE_DROP_PATH_MAX_CHARS: usize = 62;
const OPEN_WITH_PANEL_WIDTH: f32 = 420.0;
const OPEN_WITH_APPLICATION_LIST_HEIGHT: f32 = 240.0;
const OPEN_WITH_PATH_MAX_CHARS: usize = 62;
const OPEN_WITH_ERROR_MAX_CHARS: usize = 96;
const CONTEXT_MENU_WIDTH: f32 = 190.0;
const LIST_COLUMN_MENU_WIDTH: f32 = 230.0;
const CONTEXT_SUBMENU_WIDTH: f32 = 170.0;
const CONTEXT_MENU_PADDING: f32 = 8.0;
const CONTEXT_MENU_ITEM_SPACING: f32 = 4.0;
const CONTEXT_MENU_ITEM_HEIGHT: f32 = 28.0;
const LIST_COLUMN_VISIBILITY_BUTTON_HEIGHT: f32 = 28.0;
pub(super) fn error_notification_panel(error: &str, generation: u64) -> Element<'_, Message> {
    let message = crate::localization::translate_current(error);
    let message = format_middle_ellipsized_text(&message, ERROR_NOTIFICATION_MAX_CHARS);
    let close_button = button(themed_icon(IconSymbol::Close, IconTone::Normal, 13.0))
        .on_press(Message::GlobalErrorNotificationDismissed(generation))
        .padding(4)
        .width(Length::Fixed(28.0))
        .height(Length::Fixed(28.0))
        .style(navigation_icon_button_style());
    let content = row![
        themed_icon(IconSymbol::TriangleAlert, IconTone::Warning, MENU_ICON_SIZE),
        readable_text(message).size(13).width(Length::Fill),
        close_button,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    mouse_area(
        container(content)
            .padding([10, 12])
            .width(Length::Fixed(ERROR_NOTIFICATION_FLOAT_WIDTH))
            .style(error_notification_style),
    )
    .on_enter(Message::GlobalErrorNotificationPointerEntered(generation))
    .on_exit(Message::GlobalErrorNotificationPointerExited(generation))
    .on_press(Message::GlobalErrorNotificationDismissed(generation))
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

pub(super) fn destructive_action_confirmation_panel(
    confirmation: &DestructiveActionConfirmation,
) -> Element<'_, Message> {
    let (title, body, confirm_label) = match confirmation {
        DestructiveActionConfirmation::DeleteTrashEntries { entries } => {
            let item_count = entries.len();
            let item_label = if item_count == 1 {
                "1 item".to_owned()
            } else {
                format!("{item_count} items")
            };
            (
                "Delete Permanently?",
                format!("Delete {item_label} from Trash permanently? This cannot be undone."),
                "Delete Permanently",
            )
        }
        DestructiveActionConfirmation::DeletePermanently { paths } => {
            let item_count = paths.len();
            let item_label = if item_count == 1 {
                "1 item".to_owned()
            } else {
                format!("{item_count} items")
            };
            (
                "Delete Permanently?",
                format!("Delete {item_label} permanently? This cannot be undone."),
                "Delete Permanently",
            )
        }
        DestructiveActionConfirmation::EmptyTrash => (
            "Empty Trash?",
            "Delete all items in Trash permanently? This cannot be undone.".to_owned(),
            "Empty Trash",
        ),
    };

    let title_row = row![
        themed_icon(IconSymbol::TriangleAlert, IconTone::Warning, MENU_ICON_SIZE),
        readable_text(title).size(16).width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let actions = row![
        Space::new().width(Length::Fill),
        secondary_action_button("Cancel", Message::DestructiveActionCanceled),
        primary_action_button(confirm_label, Message::DestructiveActionConfirmed),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let content = column![title_row, localized_text(body).size(13), actions]
        .spacing(12)
        .width(Length::Fill);

    container(content)
        .padding(14)
        .width(Length::Fixed(DESTRUCTIVE_CONFIRMATION_PANEL_WIDTH))
        .style(context_menu_style)
        .into()
}

pub(super) fn file_drop_operation_panel(prompt: &FileDropPrompt) -> Element<'_, Message> {
    let item_count = prompt.paths.len();
    let item_label = if item_count == 1 {
        "1 item".to_owned()
    } else {
        format!("{item_count} items")
    };
    let destination = format_middle_ellipsized_text(
        prompt.paste_directory.to_string_lossy().as_ref(),
        FILE_DROP_PATH_MAX_CHARS,
    );

    let title = row![
        themed_icon(IconSymbol::ArrowRight, IconTone::Normal, MENU_ICON_SIZE),
        readable_text("Drop Files").size(16).width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let operation_choices = column![
        action_choice_row(
            "Move",
            "Move files into the current folder.",
            Message::FileDropOperationSelected(FileClipboardOperation::Move),
        ),
        action_choice_row(
            "Copy",
            "Copy files into the current folder.",
            Message::FileDropOperationSelected(FileClipboardOperation::Copy),
        ),
    ]
    .spacing(8);

    let actions = row![
        Space::new().width(Length::Fill),
        secondary_action_button("Cancel", Message::FileDropCancelled),
    ]
    .align_y(Alignment::Center);

    let content = column![
        title,
        localized_text(format!("{item_label} will be added to:")).size(13),
        readable_text(destination).size(12).width(Length::Fill),
        operation_choices,
        actions,
    ]
    .spacing(12)
    .width(Length::Fill);

    container(content)
        .padding(14)
        .width(Length::Fixed(FILE_DROP_OPERATION_PANEL_WIDTH))
        .style(context_menu_style)
        .into()
}

pub(super) fn open_with_panel(
    state: &OpenWithState,
    scrollbar_visibility: ScrollbarVisibility,
    scrollbar_viewport: Option<ScrollbarViewport>,
) -> Element<'static, Message> {
    let path_label = format_middle_ellipsized_text(
        state.path().to_string_lossy().as_ref(),
        OPEN_WITH_PATH_MAX_CHARS,
    );
    let title = row![
        themed_icon(IconSymbol::Monitor, IconTone::Normal, MENU_ICON_SIZE),
        readable_text("Open With").size(16).width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut content = column![title, readable_text(path_label).size(12)]
        .spacing(10)
        .width(Length::Fill);

    if let Some(fallback_error) = state.fallback_error() {
        content = content.push(
            localized_text(format!(
                "Default open failed: {}",
                format_middle_ellipsized_text(fallback_error, OPEN_WITH_ERROR_MAX_CHARS)
            ))
            .size(12),
        );
    }

    if let Some(mime_type) = state.mime_type() {
        content = content.push(readable_text(mime_type.to_owned()).size(12));
    }

    match state {
        OpenWithState::Loading { .. } => {
            content = content.push(readable_text("Loading applications...").size(13));
        }
        OpenWithState::Ready { .. } => {
            content = content
                .push(open_with_application_list(
                    state,
                    scrollbar_visibility,
                    scrollbar_viewport,
                ))
                .push(
                    checkbox(state.set_default_selected())
                        .label(crate::localization::translate_current(
                            "Set as default application",
                        ))
                        .on_toggle(Message::OpenWithDefaultApplicationToggled),
                );
        }
    }

    content = content.push(
        row![
            Space::new().width(Length::Fill),
            secondary_action_button("Cancel", Message::DismissFloating),
        ]
        .align_y(Alignment::Center),
    );

    container(content)
        .padding(14)
        .width(Length::Fixed(OPEN_WITH_PANEL_WIDTH))
        .style(context_menu_style)
        .into()
}

fn open_with_application_list(
    state: &OpenWithState,
    scrollbar_visibility: ScrollbarVisibility,
    scrollbar_viewport: Option<ScrollbarViewport>,
) -> Element<'static, Message> {
    let mut applications = Column::new().spacing(4);
    for application in state.applications() {
        let label = if application.is_default {
            format!(
                "{} {}",
                application.name,
                crate::localization::translate_current("(default)")
            )
        } else {
            application.name.clone()
        };
        applications = applications.push(
            button(
                row![
                    themed_icon(IconSymbol::Monitor, IconTone::Normal, MENU_ICON_SIZE),
                    column![
                        readable_text(label).size(13).width(Length::Fill),
                        readable_text(application.desktop_id.clone()).size(11),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .on_press(Message::OpenWithApplicationSelected(
                application.desktop_id.clone(),
            ))
            .padding([7, 8])
            .width(Length::Fill)
            .style(context_menu_item_button_style()),
        );
    }

    let scroll_region = ScrollbarRegion::OpenWithApplications;
    let scroller = scrollable(smooth_scroll_content(applications, scroll_region.clone()))
        .id(smooth_scroll_id(&scroll_region))
        .direction(enhanced_vertical_scrollbar_direction(
            scrollbar_visibility,
            6.0,
        ))
        .style(enhanced_scrollbar_style(scrollbar_visibility))
        .height(Length::Fixed(OPEN_WITH_APPLICATION_LIST_HEIGHT))
        .width(Length::Fill)
        .on_scroll(scrollbar_on_scroll(scroll_region.clone(), |_| {
            Message::OpenWithApplicationsScrolled
        }));

    enhanced_scrollbar(
        scroller,
        scrollbar_visibility,
        scrollbar_viewport,
        ScrollbarAxis::Vertical,
        6.0,
    )
}

fn action_label(icon: IconSymbol, label: &'static str, size: f32) -> Row<'static, Message> {
    row![
        themed_icon(icon, IconTone::Normal, size),
        crate::typography::readable_text(label)
    ]
    .spacing(6)
    .align_y(Alignment::Center)
}

pub(super) fn context_menu_panel<'a>(
    menu: &'a ContextMenuState,
    is_trash_view: bool,
    active_pane_id: BrowserPaneId,
    list_view_preferences: &'a ListViewPreferences,
    selected_search_entry_types: &'a [SearchEntryTypePreset],
) -> Element<'a, Message> {
    match menu {
        ContextMenuState::FileArea(menu) => {
            file_context_menu_panel(menu, is_trash_view, active_pane_id)
        }
        ContextMenuState::Search(menu) => search_context_menu_panel(menu),
        ContextMenuState::SearchEntryTypes(_) => {
            search_entry_types_menu_panel(selected_search_entry_types)
        }
        ContextMenuState::ListColumns(_) => list_column_context_menu_panel(list_view_preferences),
        ContextMenuState::SidebarBookmark(menu) => sidebar_bookmark_context_menu_panel(menu),
        ContextMenuState::SidebarDevice(menu) => sidebar_device_context_menu_panel(menu),
        ContextMenuState::NetworkConnection(menu) => network_connection_context_menu_panel(menu),
    }
}

fn search_entry_types_menu_panel(
    selected_entry_types: &[SearchEntryTypePreset],
) -> Element<'_, Message> {
    let mut content = Column::new()
        .spacing(CONTEXT_MENU_ITEM_SPACING + 2.0)
        .padding(CONTEXT_MENU_PADDING + 2.0);
    for entry_type in SearchEntryTypePreset::MORE {
        content = content.push(
            checkbox(selected_entry_types.contains(&entry_type))
                .label(crate::localization::translate_current(entry_type.label()))
                .on_toggle(move |_| Message::SearchEntryTypeToggled(entry_type))
                .size(16)
                .text_size(13)
                .spacing(8),
        );
    }

    container(content)
        .width(Length::Fixed(190.0))
        .style(context_menu_style)
        .into()
}

fn search_context_menu_panel(menu: &SearchContextMenuState) -> Element<'_, Message> {
    let content = Column::new()
        .spacing(CONTEXT_MENU_ITEM_SPACING)
        .padding(CONTEXT_MENU_PADDING)
        .push(menu_item(
            IconSymbol::FolderOpen,
            "Open Containing Folder",
            Message::SearchOpenContainingDirectory(menu.target.clone()),
        ))
        .push(menu_item(IconSymbol::Copy, "Copy", Message::CopySelected))
        .push(menu_item(
            IconSymbol::ArrowRight,
            "Cut",
            Message::MoveSelected,
        ))
        .push(menu_item(
            IconSymbol::Trash,
            "Move to Trash",
            Message::TrashSelected,
        ))
        .push(menu_item(
            IconSymbol::Trash,
            "Delete Permanently",
            Message::SearchDeletePermanentlySelected,
        ));

    container(content)
        .width(Length::Fixed(CONTEXT_MENU_WIDTH))
        .style(context_menu_style)
        .into()
}

fn list_column_context_menu_panel(preferences: &ListViewPreferences) -> Element<'_, Message> {
    let mut menu_content = Column::new()
        .spacing(CONTEXT_MENU_ITEM_SPACING)
        .padding(CONTEXT_MENU_PADDING);
    for column in preferences.columns() {
        menu_content = menu_content.push(list_column_menu_row(column));
    }

    container(menu_content)
        .width(Length::Fixed(LIST_COLUMN_MENU_WIDTH))
        .style(context_menu_style)
        .into()
}

fn list_column_menu_row(column: &ListColumnConfig) -> Element<'static, Message> {
    row![
        themed_icon(IconSymbol::List, IconTone::Normal, MENU_ICON_SIZE),
        readable_text(column.kind.label())
            .size(13)
            .width(Length::Fill),
        list_column_visibility_button(column),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .height(Length::Fixed(CONTEXT_MENU_ITEM_HEIGHT))
    .into()
}

fn list_column_visibility_button(column: &ListColumnConfig) -> Button<'static, Message> {
    let label = if column.kind == ListColumnKind::Name {
        "Required"
    } else if column.visible {
        "Hide"
    } else {
        "Show"
    };
    let button = button(
        container(readable_text(label).size(12))
            .center_x(Length::Fixed(62.0))
            .center_y(Length::Fixed(LIST_COLUMN_VISIBILITY_BUTTON_HEIGHT)),
    )
    .width(Length::Fixed(62.0))
    .height(Length::Fixed(LIST_COLUMN_VISIBILITY_BUTTON_HEIGHT))
    .style(context_menu_item_button_style());

    if column.kind == ListColumnKind::Name {
        button
    } else {
        button.on_press(Message::ListColumnVisibilityToggled(column.kind))
    }
}

fn file_context_menu_panel(
    menu: &FileContextMenuState,
    is_trash_view: bool,
    _active_pane_id: BrowserPaneId,
) -> Element<'_, Message> {
    if is_trash_view {
        return trash_context_menu_panel(menu);
    }

    let terminal_directory = if menu.target_is_directory {
        menu.target
            .clone()
            .unwrap_or_else(|| menu.paste_directory.clone())
    } else {
        menu.paste_directory.clone()
    };

    let mut menu_content = iced::widget::Column::new()
        .spacing(CONTEXT_MENU_ITEM_SPACING)
        .padding(CONTEXT_MENU_PADDING);
    if let Some(path) = &menu.target {
        menu_content = menu_content
            .push(menu_item(
                IconSymbol::Folder,
                "Open",
                Message::OpenPath(path.clone()),
            ))
            .push(menu_item(
                IconSymbol::Monitor,
                "Open with",
                Message::OpenWithRequested(path.clone()),
            ))
            .push(menu_item(IconSymbol::Copy, "Copy", Message::CopySelected))
            .push(menu_item(
                IconSymbol::ArrowRight,
                "Move",
                Message::MoveSelected,
            ))
            .push(menu_item(
                IconSymbol::FileArchive,
                "Create Archive...",
                Message::ArchiveCreation(ArchiveCreationMessage::OpenSelected),
            ))
            .push(menu_item(
                IconSymbol::FileImage,
                "Convert Format...",
                Message::Convert(ConvertMessage::OpenSelected),
            ));
        if !menu.target_is_directory {
            menu_content = menu_content.push(menu_item(
                IconSymbol::Hash,
                "File Checksum...",
                Message::Checksum(ChecksumMessage::OpenSelected),
            ));
        }
        menu_content = menu_content
            .push(menu_item(IconSymbol::Copy, "Paste", Message::PastePending))
            .push(menu_item(
                IconSymbol::Pencil,
                "Rename",
                Message::BeginRename(path.clone()),
            ));
        if menu.can_batch_rename {
            menu_content = menu_content.push(menu_item(
                IconSymbol::Pencil,
                "Batch Rename...",
                Message::BatchRename(BatchRenameMessage::OpenSelected),
            ));
        }
    } else {
        menu_content =
            menu_content.push(menu_item(IconSymbol::Copy, "Paste", Message::PastePending));
    }
    menu_content = menu_content.push(new_entry_menu_trigger()).push(menu_item(
        IconSymbol::Terminal,
        "Open Terminal Here",
        Message::OpenTerminalHere(terminal_directory),
    ));
    if let Some(path) = &menu.target {
        menu_content = menu_content
            .push(menu_item(
                IconSymbol::Trash,
                menu.delete_action.label(),
                Message::TrashSelected,
            ))
            .push(menu_item(
                IconSymbol::FileText,
                "Properties",
                Message::FileProperties(FilePropertiesMessage::Requested(path.clone())),
            ));
    }

    let root_menu = container(menu_content)
        .width(Length::Fixed(CONTEXT_MENU_WIDTH))
        .style(context_menu_style);

    let content = match menu.expansion {
        FileContextMenuExpansion::None => Row::new().push(root_menu),
        FileContextMenuExpansion::NewEntry => Row::new()
            .spacing(4)
            .push(root_menu)
            .push(new_entry_submenu_slot(menu)),
    };

    container(content).width(Length::Shrink).into()
}

fn new_entry_submenu_slot(menu: &FileContextMenuState) -> Element<'_, Message> {
    Column::new()
        .push(Space::new().height(Length::Fixed(new_entry_trigger_top(menu))))
        .push(new_entry_submenu(menu))
        .into()
}

fn new_entry_trigger_top(menu: &FileContextMenuState) -> f32 {
    // 文件目标比目录目标多一行 "File Checksum..."。
    let rows_before_new_entry = match (
        menu.target.is_some(),
        menu.target_is_directory,
        menu.can_batch_rename,
    ) {
        (true, false, true) => 9.0,
        (true, false, false) => 8.0,
        (true, true, true) => 8.0,
        (true, true, false) => 7.0,
        (false, _, _) => 1.0,
    };
    CONTEXT_MENU_PADDING
        + rows_before_new_entry * (CONTEXT_MENU_ITEM_HEIGHT + CONTEXT_MENU_ITEM_SPACING)
}

fn new_entry_submenu(menu: &FileContextMenuState) -> Element<'_, Message> {
    let content = Column::new()
        .spacing(CONTEXT_MENU_ITEM_SPACING)
        .padding(CONTEXT_MENU_PADDING)
        .push(submenu_item(
            IconSymbol::File,
            "New File",
            Message::CreateEmptyFile(menu.paste_directory.clone()),
        ))
        .push(submenu_item(
            IconSymbol::Folder,
            "New Folder",
            Message::CreateDirectory(menu.paste_directory.clone()),
        ));

    mouse_area(
        container(content)
            .width(Length::Fixed(CONTEXT_SUBMENU_WIDTH))
            .style(context_menu_style),
    )
    .on_enter(Message::FileContextMenuExpansionChanged(
        FileContextMenuExpansion::NewEntry,
    ))
    .into()
}

fn new_entry_menu_trigger() -> Element<'static, Message> {
    mouse_area(
        button(menu_label_with_chevron(IconSymbol::File, "New..."))
            .on_press(Message::FileContextMenuExpansionChanged(
                FileContextMenuExpansion::NewEntry,
            ))
            .width(Length::Fill)
            .height(Length::Fixed(CONTEXT_MENU_ITEM_HEIGHT))
            .style(context_menu_item_button_style()),
    )
    .on_enter(Message::FileContextMenuExpansionChanged(
        FileContextMenuExpansion::NewEntry,
    ))
    .into()
}

fn menu_item(icon: IconSymbol, label: &'static str, message: Message) -> Element<'static, Message> {
    mouse_area(menu_button(icon, label, message))
        .on_enter(Message::FileContextMenuExpansionChanged(
            FileContextMenuExpansion::None,
        ))
        .into()
}

fn submenu_item(
    icon: IconSymbol,
    label: &'static str,
    message: Message,
) -> Element<'static, Message> {
    mouse_area(menu_button(icon, label, message))
        .on_enter(Message::FileContextMenuExpansionChanged(
            FileContextMenuExpansion::NewEntry,
        ))
        .into()
}

fn sidebar_bookmark_context_menu_panel(
    menu: &SidebarBookmarkContextMenuState,
) -> Element<'_, Message> {
    let menu_content = iced::widget::Column::new()
        .spacing(4)
        .padding(8)
        .push(menu_button(
            IconSymbol::Trash,
            "Remove from Favorites",
            Message::SidebarBookmarkDeleteRequested(menu.path.clone()),
        ));

    container(menu_content)
        .width(Length::Fixed(190.0))
        .style(context_menu_style)
        .into()
}

fn sidebar_device_context_menu_panel(menu: &SidebarDeviceContextMenuState) -> Element<'_, Message> {
    let mut menu_content = iced::widget::Column::new().spacing(4).padding(8);
    let actions = menu.device.available_actions();
    if actions.is_empty() {
        menu_content = menu_content.push(readable_text("No device actions available").size(12));
    } else {
        for action in actions {
            menu_content = menu_content.push(menu_button(
                IconSymbol::HardDrive,
                action.label(&menu.device),
                Message::SidebarDeviceActionSelected(menu.device.id.clone(), action),
            ));
        }
    }

    container(menu_content)
        .width(Length::Fixed(190.0))
        .style(context_menu_style)
        .into()
}

fn trash_context_menu_panel(menu: &FileContextMenuState) -> Element<'_, Message> {
    let mut menu_content = iced::widget::Column::new().spacing(4).padding(8);
    if let Some(path) = &menu.target {
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
            ))
            .push(menu_button(
                IconSymbol::FileText,
                "Properties",
                Message::FileProperties(FilePropertiesMessage::Requested(path.clone())),
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
        .height(Length::Fixed(CONTEXT_MENU_ITEM_HEIGHT))
        .style(context_menu_item_button_style())
}

fn menu_label(icon: IconSymbol, label: &'static str) -> Row<'static, Message> {
    action_label(icon, label, MENU_ICON_SIZE).width(Length::Fill)
}

fn menu_label_with_chevron(icon: IconSymbol, label: &'static str) -> Row<'static, Message> {
    row![
        themed_icon(icon, IconTone::Normal, MENU_ICON_SIZE),
        crate::typography::readable_text(label).width(Length::Fill),
        themed_icon(IconSymbol::ChevronRight, IconTone::Normal, MENU_ICON_SIZE),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::Fill)
}
