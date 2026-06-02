use std::time::SystemTime;

use iced::widget::{
    button, column, container, mouse_area, row, scrollable, text, text_input, Button, Row, Space,
};
use iced::{Alignment, Element, Length};

use crate::app::FileBrowser;
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, context_menu_button_style,
    context_menu_style, error_notification_style, hovered_sidebar_item_style,
    navigation_icon_button_style, selected_sidebar_item_style, sidebar_style, switch_thumb_style,
    switch_track_off_style, switch_track_on_style,
};
use crate::config::COLUMN_FIXED_COUNT_OPTIONS;
use crate::formatting::{format_file_size, format_middle_ellipsized_text};
use crate::icons::IconSymbol;
use crate::model::{
    trash_location_path, ColumnViewMode, ContextMenuState, Message, SidebarLocation,
    TransferConflictChoice, TransferConflictItem, TransferConflictMetadata, TransferConflictState,
    TRASH_LOCATION_LABEL,
};
use crate::sidebar::SIDEBAR_WIDTH;
use crate::typography::readable_text;

use super::{themed_icon, IconTone, MENU_ICON_SIZE};

const COLUMN_SETTINGS_FLOAT_WIDTH: f32 = 260.0;
const ERROR_NOTIFICATION_FLOAT_WIDTH: f32 = 560.0;
const ERROR_NOTIFICATION_MAX_CHARS: usize = 96;
const TRANSFER_CONFLICT_PANEL_WIDTH: f32 = 560.0;
const TRANSFER_CONFLICT_PATH_MAX_CHARS: usize = 68;
const SIDEBAR_LABEL_MAX_CHARS: usize = 22;

pub(super) fn error_notification_panel(error: &str) -> Element<'_, Message> {
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

pub(super) fn sidebar_view(browser: &FileBrowser) -> Element<'_, Message> {
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

    container(
        scrollable(sidebar)
            .direction(auto_hide_vertical_scrollbar_direction(
                browser.scrollbar_visibility,
                6.0,
            ))
            .style(auto_hide_scrollbar_style(browser.scrollbar_visibility))
            .height(Length::Fill),
    )
    .width(Length::Fixed(SIDEBAR_WIDTH))
    .height(Length::Fill)
    .style(sidebar_style)
    .into()
}

pub(super) fn column_settings_panel(browser: &FileBrowser) -> Element<'_, Message> {
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

pub(super) fn context_menu_panel(
    menu: &ContextMenuState,
    is_trash_view: bool,
) -> Element<'_, Message> {
    if is_trash_view {
        return trash_context_menu_panel(menu);
    }

    let paste_button = menu_button(IconSymbol::Copy, "Paste", Message::PastePending);

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
    let mut menu_content = iced::widget::Column::new().spacing(4).padding(8);
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
