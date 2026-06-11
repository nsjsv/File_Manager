use std::time::SystemTime;

use desktop_linux::{TerminalEmulator, TERMINAL_EMULATOR_OPTIONS};
use iced::widget::{
    button, column, container, mouse_area, row, scrollable, text, text_input, Button, Row, Space,
};
use iced::{Alignment, Element, Length};

use crate::app::FileBrowser;
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, context_menu_button_style,
    context_menu_style, error_notification_style, hovered_sidebar_item_style,
    navigation_icon_button_style, selected_sidebar_item_style, sidebar_bookmark_drop_slot_style,
    sidebar_style,
};
use crate::config;
use crate::formatting::{format_file_size, format_middle_ellipsized_text};
use crate::icons::IconSymbol;
use crate::model::{
    trash_location_path, BrowserPaneId, ContextMenuState, DestructiveActionConfirmation,
    FileContextMenuState, Message, SidebarBookmarkContextMenuState, SidebarBookmarkDropSlot,
    SidebarLocation, SidebarLocationKind, TransferConflictChoice, TransferConflictItem,
    TransferConflictMetadata, TransferConflictState, TRASH_LOCATION_LABEL,
};
use crate::typography::readable_text;

use super::rendering_settings::rendering_gpu_preference_button;
use super::toggle_switch::switch_control;
use super::{tab_motion, themed_icon, IconTone, MENU_ICON_SIZE};

const COLUMN_SETTINGS_FLOAT_WIDTH: f32 = 260.0;
const ERROR_NOTIFICATION_FLOAT_WIDTH: f32 = 560.0;
const ERROR_NOTIFICATION_MAX_CHARS: usize = 96;
const DESTRUCTIVE_CONFIRMATION_PANEL_WIDTH: f32 = 460.0;
const TRANSFER_CONFLICT_PANEL_WIDTH: f32 = 560.0;
const TRANSFER_CONFLICT_PATH_MAX_CHARS: usize = 68;
const SIDEBAR_LABEL_REFERENCE_MAX_CHARS: usize = 22;
const SIDEBAR_LABEL_MIN_CHARS: usize = 14;
const SIDEBAR_LABEL_MAX_CHARS: usize = 44;
const SIDEBAR_RESIZE_HANDLE_WIDTH: f32 = 6.0;
const SIDEBAR_ITEM_VERTICAL_PADDING: f32 = 12.0;
const SIDEBAR_BOOKMARK_DROP_SLOT_HEIGHT: f32 = MENU_ICON_SIZE + SIDEBAR_ITEM_VERTICAL_PADDING;

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
    .align_y(Alignment::Center);

    let mut sidebar = column![sidebar_header].spacing(6).padding(12);
    for location in browser
        .sidebar_locations
        .iter()
        .filter(|location| !location.kind.is_user_favorite())
    {
        sidebar = sidebar.push(sidebar_location_item(browser, location));
    }
    sidebar = sidebar.push(sidebar_trash_item(browser));

    let favorite_locations = browser
        .sidebar_locations
        .iter()
        .filter(|location| location.kind.is_user_favorite())
        .collect::<Vec<_>>();
    let can_drop_bookmark = browser.can_drop_sidebar_bookmark();
    if can_drop_bookmark || !favorite_locations.is_empty() {
        sidebar = sidebar.push(sidebar_section_label("Favorites"));

        if can_drop_bookmark
            && browser.sidebar_bookmark_drop_slot == Some(SidebarBookmarkDropSlot::Top)
        {
            sidebar = sidebar.push(sidebar_bookmark_drop_slot(
                browser,
                SidebarBookmarkDropSlot::Top,
            ));
        }

        for location in favorite_locations {
            sidebar = sidebar.push(sidebar_location_item(browser, location));
        }

        if can_drop_bookmark
            && browser.sidebar_bookmark_drop_slot == Some(SidebarBookmarkDropSlot::Bottom)
        {
            sidebar = sidebar.push(sidebar_bookmark_drop_slot(
                browser,
                SidebarBookmarkDropSlot::Bottom,
            ));
        }
    }

    let sidebar_scroller = scrollable(sidebar)
        .direction(auto_hide_vertical_scrollbar_direction(
            browser.scrollbar_visibility,
            6.0,
        ))
        .style(auto_hide_scrollbar_style(browser.scrollbar_visibility))
        .height(Length::Fill);
    let sidebar_content_panel = container(sidebar_scroller)
        .width(Length::Fill)
        .height(Length::Fill);

    let sidebar_content: Element<'_, Message> = if can_drop_bookmark {
        mouse_area(sidebar_content_panel)
            .on_move(Message::SidebarPointerMoved)
            .on_exit(Message::SidebarPointerExited)
            .into()
    } else {
        sidebar_content_panel.into()
    };

    container(
        row![sidebar_content, sidebar_resize_handle()]
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fixed(browser.sidebar_width))
    .height(Length::Fill)
    .style(sidebar_style)
    .into()
}

fn sidebar_resize_handle() -> Element<'static, Message> {
    let handle = container(Space::new().width(Length::Fixed(SIDEBAR_RESIZE_HANDLE_WIDTH)))
        .width(Length::Fixed(SIDEBAR_RESIZE_HANDLE_WIDTH))
        .height(Length::Fill);

    mouse_area(handle)
        .on_press(Message::SidebarResizeStarted)
        .on_release(Message::DragSelectionFinished)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into()
}

fn sidebar_location_item<'a>(
    browser: &'a FileBrowser,
    location: &'a SidebarLocation,
) -> Element<'a, Message> {
    let presentation = sidebar_presentation(browser, location);
    let tone = if presentation.is_selected() {
        IconTone::Selected
    } else {
        IconTone::Normal
    };

    let item_container = container(sidebar_label(
        sidebar_icon_symbol(location),
        &location.label,
        tone,
        sidebar_label_max_chars(browser.sidebar_width),
    ))
    .padding([6, 8])
    .width(Length::Fill);
    let item_container = match presentation {
        SidebarPresentation::Selected => item_container.style(selected_sidebar_item_style),
        SidebarPresentation::Hovered => item_container.style(hovered_sidebar_item_style),
        SidebarPresentation::Normal => item_container,
    };

    let is_favorite = location.kind.is_user_favorite();
    let item_content: Element<'a, Message> = if is_favorite {
        tab_motion::translated(
            item_container,
            0.0,
            browser.sidebar_bookmark_motion_offset(location.path.as_path()),
        )
    } else {
        item_container.into()
    };

    let item = mouse_area(item_content)
        .on_enter(if is_favorite {
            Message::SidebarBookmarkEntered(location.path.clone())
        } else {
            Message::SidebarHovered(location.path.clone())
        })
        .on_exit(Message::SidebarHoverCleared(location.path.clone()))
        .on_middle_press(Message::OpenDirectoryFromMiddleClick(
            browser.active_pane_id(),
            location.path.clone(),
        ))
        .on_press(if is_favorite {
            Message::SidebarBookmarkPressed(location.path.clone())
        } else {
            Message::NavigateTo(location.path.clone())
        })
        .on_release(if is_favorite {
            Message::SidebarBookmarkReleased
        } else {
            Message::DragSelectionFinished
        })
        .interaction(iced::mouse::Interaction::Pointer);
    let item = if is_favorite {
        item.on_right_press(Message::SidebarBookmarkRightClicked(location.path.clone()))
    } else {
        item
    };

    item.into()
}

fn sidebar_trash_item(browser: &FileBrowser) -> Element<'_, Message> {
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
        sidebar_label_max_chars(browser.sidebar_width),
    ))
    .padding([6, 8])
    .width(Length::Fill);
    let trash_container = match trash_presentation {
        SidebarPresentation::Selected => trash_container.style(selected_sidebar_item_style),
        SidebarPresentation::Hovered => trash_container.style(hovered_sidebar_item_style),
        SidebarPresentation::Normal => trash_container,
    };
    let trash_hover_path = trash_path.clone();
    mouse_area(trash_container)
        .on_enter(Message::SidebarHovered(trash_hover_path.clone()))
        .on_exit(Message::SidebarHoverCleared(trash_hover_path))
        .on_press(Message::TrashOpened)
        .on_middle_press(Message::OpenTrashInNewTab(browser.active_pane_id()))
        .on_release(Message::DragSelectionFinished)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

fn sidebar_section_label(label: &'static str) -> Element<'static, Message> {
    container(readable_text(label).size(12))
        .padding([4, 8])
        .width(Length::Fill)
        .into()
}

fn sidebar_bookmark_drop_slot(
    browser: &FileBrowser,
    slot: SidebarBookmarkDropSlot,
) -> Element<'_, Message> {
    let is_active = browser.sidebar_bookmark_drop_slot == Some(slot);
    let content = container(Space::new().height(Length::Fixed(SIDEBAR_BOOKMARK_DROP_SLOT_HEIGHT)))
        .height(Length::Fixed(SIDEBAR_BOOKMARK_DROP_SLOT_HEIGHT))
        .width(Length::Fill);
    let content = if is_active {
        content.style(sidebar_bookmark_drop_slot_style)
    } else {
        content
    };

    mouse_area(content)
        .on_enter(Message::SidebarBookmarkDropSlotHovered(slot))
        .on_exit(Message::SidebarBookmarkDropSlotCleared(slot))
        .on_release(Message::DragSelectionFinished)
        .into()
}

fn sidebar_icon_symbol(location: &SidebarLocation) -> IconSymbol {
    match location.kind {
        SidebarLocationKind::Home => IconSymbol::House,
        SidebarLocationKind::Desktop => IconSymbol::Monitor,
        SidebarLocationKind::Documents => IconSymbol::FileText,
        SidebarLocationKind::Downloads => IconSymbol::Download,
        SidebarLocationKind::Pictures => IconSymbol::FileImage,
        SidebarLocationKind::Music => IconSymbol::Music,
        SidebarLocationKind::Videos => IconSymbol::Video,
        SidebarLocationKind::Bookmark => IconSymbol::Bookmark,
    }
}

pub(super) fn column_settings_panel(browser: &FileBrowser) -> Element<'_, Message> {
    container(
        column![
            readable_text("Settings").size(16),
            readable_text("Files").size(13),
            hidden_files_visibility_button(browser),
            readable_text("Rendering").size(13),
            rendering_gpu_preference_button(browser.rendering_gpu_preference),
            readable_text("Terminal").size(13),
            terminal_emulator_options(browser.terminal_emulator),
        ]
        .spacing(6),
    )
    .padding(14)
    .width(Length::Fixed(COLUMN_SETTINGS_FLOAT_WIDTH))
    .style(context_menu_style)
    .into()
}

fn terminal_emulator_options(selected: TerminalEmulator) -> Element<'static, Message> {
    let mut options = iced::widget::Column::new().spacing(4);
    for terminal_emulator in TERMINAL_EMULATOR_OPTIONS {
        options = options.push(terminal_emulator_button(*terminal_emulator, selected));
    }
    options.into()
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
    .align_y(Alignment::Center);

    button(container(label).padding([5, 8]).width(Length::Fill))
        .on_press(Message::ShowHiddenFilesToggled)
        .width(Length::Fill)
        .style(context_menu_button_style())
}

fn terminal_emulator_button(
    terminal_emulator: TerminalEmulator,
    selected_emulator: TerminalEmulator,
) -> Button<'static, Message> {
    let label = container(readable_text(terminal_emulator.label()).size(12))
        .padding([5, 8])
        .width(Length::Fill);
    let label = if terminal_emulator == selected_emulator {
        label.style(selected_sidebar_item_style)
    } else {
        label
    };

    button(label)
        .on_press(Message::TerminalEmulatorSelected(terminal_emulator))
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

fn sidebar_label(
    icon: IconSymbol,
    label: &str,
    tone: IconTone,
    max_chars: usize,
) -> Row<'static, Message> {
    let label = format_middle_ellipsized_text(label, max_chars);
    row![
        themed_icon(icon, tone, MENU_ICON_SIZE),
        readable_text(label).width(Length::Fill)
    ]
    .spacing(8)
    .align_y(Alignment::Center)
}

fn sidebar_label_max_chars(sidebar_width: f32) -> usize {
    let content_width = (sidebar_width - SIDEBAR_RESIZE_HANDLE_WIDTH).max(1.0);
    let scaled_chars =
        SIDEBAR_LABEL_REFERENCE_MAX_CHARS as f32 * content_width / config::DEFAULT_SIDEBAR_WIDTH;
    (scaled_chars.round() as usize).clamp(SIDEBAR_LABEL_MIN_CHARS, SIDEBAR_LABEL_MAX_CHARS)
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

fn trash_context_menu_panel(menu: &FileContextMenuState) -> Element<'_, Message> {
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
