use std::time::SystemTime;

use iced::widget::{
    button, checkbox, column, container, row, scrollable, text, text_input, Button, Column, Row,
    Space,
};
use iced::{Alignment, Element, Length};

use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, context_menu_button_style,
    context_menu_style, error_notification_style,
};
use crate::formatting::{format_file_size, format_middle_ellipsized_text};
use crate::icons::IconSymbol;
use crate::model::{
    BrowserPaneId, ContextMenuState, DestructiveActionConfirmation, FileContextMenuState, Message,
    ScrollbarVisibility, SidebarBookmarkContextMenuState, TransferConflictChoice,
    TransferConflictItem, TransferConflictMetadata, TransferConflictState,
};
use crate::open_with::OpenWithState;
use crate::sidebar_devices::SidebarDeviceContextMenuState;
use crate::typography::readable_text;

use super::{themed_icon, IconTone, MENU_ICON_SIZE};

const ERROR_NOTIFICATION_FLOAT_WIDTH: f32 = 560.0;
const ERROR_NOTIFICATION_MAX_CHARS: usize = 96;
const DESTRUCTIVE_CONFIRMATION_PANEL_WIDTH: f32 = 460.0;
const TRANSFER_CONFLICT_PANEL_WIDTH: f32 = 560.0;
const TRANSFER_CONFLICT_PATH_MAX_CHARS: usize = 68;
const OPEN_WITH_PANEL_WIDTH: f32 = 420.0;
const OPEN_WITH_APPLICATION_LIST_HEIGHT: f32 = 240.0;
const OPEN_WITH_PATH_MAX_CHARS: usize = 62;
const OPEN_WITH_ERROR_MAX_CHARS: usize = 96;
pub(super) fn error_notification_panel(error: &str) -> Element<'_, Message> {
    let message = format_middle_ellipsized_text(error, ERROR_NOTIFICATION_MAX_CHARS);
    let content = row![
        themed_icon(IconSymbol::TriangleAlert, IconTone::Warning, MENU_ICON_SIZE),
        readable_text(message).size(13).width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    container(content)
        .padding([10, 12])
        .width(Length::Fixed(ERROR_NOTIFICATION_FLOAT_WIDTH))
        .style(error_notification_style)
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
        button(readable_text("Cancel").size(12))
            .on_press(Message::DestructiveActionCanceled)
            .padding([6, 10])
            .style(context_menu_button_style()),
        button(readable_text(confirm_label).size(12))
            .on_press(Message::DestructiveActionConfirmed)
            .padding([6, 10])
            .style(context_menu_button_style()),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let content = column![title_row, readable_text(body).size(13), actions]
        .spacing(12)
        .width(Length::Fill);

    container(content)
        .padding(14)
        .width(Length::Fixed(DESTRUCTIVE_CONFIRMATION_PANEL_WIDTH))
        .style(context_menu_style)
        .into()
}

pub(super) fn transfer_conflict_panel(state: &TransferConflictState) -> Element<'_, Message> {
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
    .align_y(Alignment::Center);

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
    .align_y(Alignment::Center);
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
    .align_y(Alignment::Center);

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

pub(super) fn open_with_panel(
    state: &OpenWithState,
    scrollbar_visibility: ScrollbarVisibility,
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
            readable_text(format!(
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
                .push(open_with_application_list(state, scrollbar_visibility))
                .push(
                    checkbox(state.set_default_selected())
                        .label("Set as default application")
                        .on_toggle(Message::OpenWithDefaultApplicationToggled),
                );
        }
    }

    content = content.push(
        row![
            Space::new().width(Length::Fill),
            button(readable_text("Cancel").size(12))
                .on_press(Message::DismissFloating)
                .padding([6, 10])
                .style(context_menu_button_style()),
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
) -> Element<'static, Message> {
    let mut applications = Column::new().spacing(4);
    for application in state.applications() {
        let label = if application.is_default {
            format!("{} (default)", application.name)
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
            .style(context_menu_button_style()),
        );
    }

    scrollable(applications)
        .direction(auto_hide_vertical_scrollbar_direction(
            scrollbar_visibility,
            6.0,
        ))
        .style(auto_hide_scrollbar_style(scrollbar_visibility))
        .height(Length::Fixed(OPEN_WITH_APPLICATION_LIST_HEIGHT))
        .width(Length::Fill)
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
        .align_y(Alignment::Center)
}

pub(super) fn context_menu_panel(
    menu: &ContextMenuState,
    is_trash_view: bool,
    active_pane_id: BrowserPaneId,
) -> Element<'_, Message> {
    match menu {
        ContextMenuState::FileArea(menu) => {
            file_context_menu_panel(menu, is_trash_view, active_pane_id)
        }
        ContextMenuState::SidebarBookmark(menu) => sidebar_bookmark_context_menu_panel(menu),
        ContextMenuState::SidebarDevice(menu) => sidebar_device_context_menu_panel(menu),
    }
}

fn file_context_menu_panel(
    menu: &FileContextMenuState,
    is_trash_view: bool,
    active_pane_id: BrowserPaneId,
) -> Element<'_, Message> {
    if is_trash_view {
        return trash_context_menu_panel(menu);
    }

    let paste_button = menu_button(IconSymbol::Copy, "Paste", Message::PastePending);
    let terminal_directory = if menu.target_is_directory {
        menu.target
            .clone()
            .unwrap_or_else(|| menu.paste_directory.clone())
    } else {
        menu.paste_directory.clone()
    };

    let mut menu_content = iced::widget::Column::new().spacing(4).padding(8);
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
        ))
        .push(menu_button(
            IconSymbol::Terminal,
            "Open Terminal Here",
            Message::OpenTerminalHere(terminal_directory),
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
                Message::OpenDirectoryInNewTab(active_pane_id, path.clone()),
            ));
        }
        menu_content = menu_content.push(menu_button(
            IconSymbol::Monitor,
            "Open With...",
            Message::OpenWithRequested(path.clone()),
        ));
        menu_content = menu_content.push(menu_button(
            IconSymbol::FileText,
            "Properties",
            Message::FilePropertiesRequested(path.clone()),
        ));
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
                IconSymbol::FileText,
                "Properties",
                Message::FilePropertiesRequested(path.clone()),
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
