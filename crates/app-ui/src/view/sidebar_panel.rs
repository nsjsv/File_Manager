use iced::widget::{
    button, column, container, mouse_area, row, scrollable, stack, text, Column, Row, Space,
};
use iced::{Alignment, Element, Length, Padding};

use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use crate::app::FileBrowser;
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, hovered_sidebar_item_style,
    navigation_icon_button_style, selected_sidebar_item_style, sidebar_bookmark_drop_slot_style,
    sidebar_style,
};
use crate::config;
use crate::formatting::{format_file_size, format_middle_ellipsized_text};
use crate::icons::IconSymbol;
use crate::model::{
    trash_location_path, Message, ScrollbarRegion, SidebarBookmarkDropSlot, SidebarLocation,
    SidebarLocationKind, TRASH_LOCATION_LABEL,
};
use crate::network_connections::{NetworkConnectionMessage, SidebarNetworkConnectionEntry};
use crate::sidebar_devices::SidebarDeviceEntry;
use crate::typography::readable_text;

use super::{tab_motion, themed_icon, IconTone, MENU_ICON_SIZE};

const SIDEBAR_LABEL_REFERENCE_MAX_CHARS: usize = 22;
const SIDEBAR_LABEL_MIN_CHARS: usize = 14;
const SIDEBAR_LABEL_MAX_CHARS: usize = 44;
const SIDEBAR_RESIZE_HANDLE_WIDTH: f32 = 6.0;
const SIDEBAR_FLOATING_MARGIN_LEFT: f32 = 10.0;
const SIDEBAR_FLOATING_MARGIN_RIGHT: f32 = 4.0;
const SIDEBAR_FLOATING_MARGIN_VERTICAL: f32 = 10.0;
const SIDEBAR_BOOKMARK_DROP_SLOT_HEIGHT: f32 = 3.0;

pub(crate) fn sidebar_view(browser: &FileBrowser) -> Element<'_, Message> {
    let sidebar_header = row![
        text("Places").size(16).width(Length::Fill),
        button(themed_icon(
            IconSymbol::Settings,
            IconTone::Normal,
            MENU_ICON_SIZE
        ))
        .on_press(Message::SettingsOpened)
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

        for (index, location) in favorite_locations.iter().enumerate() {
            sidebar = sidebar.push(sidebar_location_item_with_index(browser, location, index));
        }
    }

    sidebar = append_sidebar_network_connections(sidebar, browser);
    sidebar = append_sidebar_devices(sidebar, browser);

    let scrollbar_region = ScrollbarRegion::Sidebar;
    let scrollbar_visibility = browser.scrollbar_visibility_for(&scrollbar_region);
    let sidebar_scroller = scrollable(smooth_scroll_content(sidebar, scrollbar_region.clone()))
        .id(smooth_scroll_id(&scrollbar_region))
        .direction(auto_hide_vertical_scrollbar_direction(
            scrollbar_visibility,
            6.0,
        ))
        .style(auto_hide_scrollbar_style(scrollbar_visibility))
        .height(Length::Fill)
        .on_scroll(|_| Message::SidebarScrolled);
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

    let sidebar_panel = container(sidebar_content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(sidebar_style);

    let sidebar_panel = container(sidebar_panel)
        .padding(sidebar_floating_panel_margin())
        .width(Length::Fill)
        .height(Length::Fill);

    container(
        row![sidebar_panel, sidebar_resize_handle()]
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fixed(browser.sidebar_width))
    .height(Length::Fill)
    .into()
}

pub(super) fn sidebar_floating_panel_margin() -> Padding {
    Padding {
        top: SIDEBAR_FLOATING_MARGIN_VERTICAL,
        right: SIDEBAR_FLOATING_MARGIN_RIGHT,
        bottom: SIDEBAR_FLOATING_MARGIN_VERTICAL,
        left: SIDEBAR_FLOATING_MARGIN_LEFT,
    }
}

fn append_sidebar_devices<'a>(
    mut sidebar: Column<'a, Message>,
    browser: &'a FileBrowser,
) -> Column<'a, Message> {
    if browser.sidebar_devices.devices.is_empty()
        && browser.sidebar_devices.unavailable.is_none()
        && !browser.sidebar_devices.is_loading
    {
        return sidebar;
    }

    sidebar = sidebar.push(sidebar_section_label("Devices"));
    for device in &browser.sidebar_devices.devices {
        sidebar = sidebar.push(sidebar_device_item(browser, device));
    }

    if browser.sidebar_devices.devices.is_empty() && browser.sidebar_devices.is_loading {
        sidebar = sidebar.push(sidebar_message_row("Loading devices..."));
    } else if browser.sidebar_devices.devices.is_empty() {
        if browser.sidebar_devices.unavailable.is_some() {
            sidebar = sidebar.push(sidebar_message_row("Devices unavailable"));
        }
    }

    sidebar
}

fn append_sidebar_network_connections<'a>(
    mut sidebar: Column<'a, Message>,
    browser: &'a FileBrowser,
) -> Column<'a, Message> {
    sidebar = sidebar.push(sidebar_network_section_header());
    for connection in &browser.network_connections.entries {
        sidebar = sidebar.push(sidebar_network_connection_item(browser, connection));
    }

    if browser.network_connections.entries.is_empty() {
        sidebar = sidebar.push(sidebar_message_row("No saved connections"));
    } else if browser.network_connections.unavailable.is_some() {
        sidebar = sidebar.push(sidebar_message_row("Network status unavailable"));
    }

    sidebar
}

fn sidebar_network_section_header() -> Element<'static, Message> {
    row![
        container(readable_text("Network").size(12))
            .padding([4, 8])
            .width(Length::Fill),
        button(themed_icon(
            IconSymbol::Plus,
            IconTone::Normal,
            MENU_ICON_SIZE
        ))
        .on_press(Message::NetworkConnection(
            NetworkConnectionMessage::AddRequested
        ))
        .padding([2, 5])
        .style(navigation_icon_button_style()),
    ]
    .align_y(Alignment::Center)
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
    sidebar_location_item_content(browser, location, None)
}

fn sidebar_location_item_with_index<'a>(
    browser: &'a FileBrowser,
    location: &'a SidebarLocation,
    favorite_index: usize,
) -> Element<'a, Message> {
    sidebar_location_item_content(browser, location, Some(favorite_index))
}

fn sidebar_location_item_content<'a>(
    browser: &'a FileBrowser,
    location: &'a SidebarLocation,
    favorite_index: Option<usize>,
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

    let is_favorite = favorite_index.is_some();
    let item_container = if let Some(favorite_index) = favorite_index {
        sidebar_bookmark_drop_overlay(browser, item_container, favorite_index)
    } else {
        item_container.into()
    };
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
        .on_release(if is_favorite && browser.file_drag.is_none() {
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

fn sidebar_bookmark_drop_overlay<'a>(
    browser: &FileBrowser,
    item_container: iced::widget::Container<'a, Message>,
    favorite_index: usize,
) -> Element<'a, Message> {
    let Some(slot) = browser.sidebar_bookmark_drop_slot else {
        return item_container.into();
    };

    let SidebarBookmarkDropSlot::Insert { index } = slot;
    let line_alignment = if index == 0 && favorite_index == 0 {
        SidebarBookmarkDropLineAlignment::Top
    } else if index > 0 && index == favorite_index + 1 {
        SidebarBookmarkDropLineAlignment::Bottom
    } else {
        return item_container.into();
    };

    let line = container(Space::new().height(Length::Fixed(SIDEBAR_BOOKMARK_DROP_SLOT_HEIGHT)))
        .height(Length::Fixed(SIDEBAR_BOOKMARK_DROP_SLOT_HEIGHT))
        .width(Length::Fill)
        .style(sidebar_bookmark_drop_slot_style);
    let aligned_line = match line_alignment {
        SidebarBookmarkDropLineAlignment::Top => container(line).align_top(Length::Fill),
        SidebarBookmarkDropLineAlignment::Bottom => container(line).align_bottom(Length::Fill),
    };

    stack([item_container.into(), aligned_line.into()])
        .width(Length::Fill)
        .into()
}

enum SidebarBookmarkDropLineAlignment {
    Top,
    Bottom,
}

fn sidebar_device_item<'a>(
    browser: &'a FileBrowser,
    device: &'a SidebarDeviceEntry,
) -> Element<'a, Message> {
    let presentation = sidebar_device_presentation(browser, device);
    let tone = if presentation.is_selected() {
        IconTone::Selected
    } else {
        IconTone::Normal
    };
    let pending = browser.sidebar_devices.pending_action.as_ref() == Some(&device.id);

    let item_container = container(sidebar_device_label(
        device,
        pending,
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

    mouse_area(item_container)
        .on_enter(Message::SidebarDeviceHovered(device.id.clone()))
        .on_exit(Message::SidebarDeviceHoverCleared(device.id.clone()))
        .on_middle_press(Message::SidebarDeviceMiddlePressed(
            browser.active_pane_id(),
            device.id.clone(),
        ))
        .on_press(Message::SidebarDevicePressed(device.id.clone()))
        .on_right_press(Message::SidebarDeviceRightClicked(device.id.clone()))
        .on_release(Message::DragSelectionFinished)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

fn sidebar_network_connection_item<'a>(
    browser: &'a FileBrowser,
    connection: &'a SidebarNetworkConnectionEntry,
) -> Element<'a, Message> {
    let presentation = sidebar_network_connection_presentation(browser, connection);
    let tone = if presentation.is_selected() {
        IconTone::Selected
    } else {
        IconTone::Normal
    };
    let pending = browser.network_connections.is_pending(connection.id());

    let item_container = container(sidebar_network_connection_label(
        connection,
        pending,
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

    mouse_area(item_container)
        .on_enter(Message::NetworkConnection(
            NetworkConnectionMessage::Hovered(connection.id().clone()),
        ))
        .on_exit(Message::NetworkConnection(
            NetworkConnectionMessage::HoverCleared(connection.id().clone()),
        ))
        .on_middle_press(Message::NetworkConnection(
            NetworkConnectionMessage::MiddlePressed(
                browser.active_pane_id(),
                connection.id().clone(),
            ),
        ))
        .on_press(Message::NetworkConnection(
            NetworkConnectionMessage::Pressed(connection.id().clone()),
        ))
        .on_right_press(Message::NetworkConnection(
            NetworkConnectionMessage::RightClicked(connection.id().clone()),
        ))
        .on_release(Message::DragSelectionFinished)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
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

fn sidebar_message_row(message: &'static str) -> Element<'static, Message> {
    container(readable_text(message).size(12))
        .padding([4, 8])
        .width(Length::Fill)
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

fn sidebar_presentation(browser: &FileBrowser, location: &SidebarLocation) -> SidebarPresentation {
    if !browser.is_trash_view && location.path == browser.current_dir {
        SidebarPresentation::Selected
    } else if browser.hovered_sidebar.as_ref() == Some(&location.path) {
        SidebarPresentation::Hovered
    } else {
        SidebarPresentation::Normal
    }
}

fn sidebar_device_presentation(
    browser: &FileBrowser,
    device: &SidebarDeviceEntry,
) -> SidebarPresentation {
    if browser.sidebar_device_is_selected(&device.id) {
        SidebarPresentation::Selected
    } else if browser.hovered_sidebar_device.as_ref() == Some(&device.id) {
        SidebarPresentation::Hovered
    } else {
        SidebarPresentation::Normal
    }
}

fn sidebar_network_connection_presentation(
    browser: &FileBrowser,
    connection: &SidebarNetworkConnectionEntry,
) -> SidebarPresentation {
    if browser.network_connection_is_selected(connection.id()) {
        SidebarPresentation::Selected
    } else if browser.hovered_network_connection.as_ref() == Some(connection.id()) {
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

fn sidebar_device_label(
    device: &SidebarDeviceEntry,
    pending: bool,
    tone: IconTone,
    max_chars: usize,
) -> Row<'static, Message> {
    let label = format_middle_ellipsized_text(&device.label, max_chars);
    let detail = format_middle_ellipsized_text(&sidebar_device_detail(device, pending), max_chars);
    row![
        themed_icon(IconSymbol::HardDrive, tone, MENU_ICON_SIZE),
        column![
            readable_text(label).size(13),
            readable_text(detail).size(11)
        ]
        .spacing(1)
        .width(Length::Fill)
    ]
    .spacing(8)
    .align_y(Alignment::Center)
}

fn sidebar_network_connection_label(
    connection: &SidebarNetworkConnectionEntry,
    pending: bool,
    tone: IconTone,
    max_chars: usize,
) -> Row<'static, Message> {
    let label = format_middle_ellipsized_text(&connection.label(), max_chars);
    let detail = format_middle_ellipsized_text(
        &sidebar_network_connection_detail(connection, pending),
        max_chars,
    );
    row![
        themed_icon(IconSymbol::Link, tone, MENU_ICON_SIZE),
        column![
            readable_text(label).size(13),
            readable_text(detail).size(11)
        ]
        .spacing(1)
        .width(Length::Fill)
    ]
    .spacing(8)
    .align_y(Alignment::Center)
}

fn sidebar_network_connection_detail(
    connection: &SidebarNetworkConnectionEntry,
    pending: bool,
) -> String {
    if pending {
        "Working...".to_owned()
    } else {
        match &connection.state {
            desktop_linux::NetworkMountState::Disconnected => "Not connected".to_owned(),
            desktop_linux::NetworkMountState::Connecting => "Connecting...".to_owned(),
            desktop_linux::NetworkMountState::Mounted(path) => path.to_string_lossy().into_owned(),
            desktop_linux::NetworkMountState::Error(_) => "Connection error".to_owned(),
        }
    }
}

fn sidebar_device_detail(device: &SidebarDeviceEntry, pending: bool) -> String {
    if pending {
        "Working...".to_owned()
    } else if device.is_mounted() {
        device
            .detail
            .clone()
            .unwrap_or_else(|| "Mounted".to_owned())
    } else if device.size_bytes > 0 {
        format!("Not mounted · {}", format_file_size(device.size_bytes))
    } else {
        "Not mounted".to_owned()
    }
}

fn sidebar_label_max_chars(sidebar_width: f32) -> usize {
    let content_width = (sidebar_width
        - SIDEBAR_RESIZE_HANDLE_WIDTH
        - SIDEBAR_FLOATING_MARGIN_LEFT
        - SIDEBAR_FLOATING_MARGIN_RIGHT)
        .max(1.0);
    let scaled_chars =
        SIDEBAR_LABEL_REFERENCE_MAX_CHARS as f32 * content_width / config::DEFAULT_SIDEBAR_WIDTH;
    (scaled_chars.round() as usize).clamp(SIDEBAR_LABEL_MIN_CHARS, SIDEBAR_LABEL_MAX_CHARS)
}
