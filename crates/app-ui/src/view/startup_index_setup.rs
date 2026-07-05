use iced::widget::{checkbox, column, container, mouse_area, row, scrollable, Column, Space};
use iced::{Alignment, Element, Length};

use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, context_menu_style,
};
use crate::formatting::format_middle_ellipsized_text;
use crate::icons::{preview_entry_icon_symbol, rotated_chevron_right_view};
use crate::model::{
    Message, ScrollbarRegion, ScrollbarVisibility, StartupIndexCapability,
    StartupIndexDirectoryChildren, StartupIndexSetupState, StartupIndexTargetMode,
    StartupIndexTreeEntry,
};
use crate::typography::readable_text;

use super::option_controls::{
    inactive_primary_action_button, primary_action_button, selectable_choice_row,
};
use super::toggle_switch::switch_control;
use super::{icon_tone_style, themed_icon, IconTone};

const STARTUP_INDEX_PANEL_WIDTH: f32 = 520.0;
const STARTUP_INDEX_TREE_HEIGHT: f32 = 300.0;
const STARTUP_INDEX_TREE_INDENT_WIDTH: f32 = 18.0;
const STARTUP_INDEX_TREE_TOGGLE_WIDTH: f32 = 16.0;
const STARTUP_INDEX_TREE_TOGGLE_ROTATION_DEGREES: f32 = 90.0;
const STARTUP_INDEX_ICON_SIZE: f32 = 16.0;
const STARTUP_INDEX_NAME_MAX_CHARS: usize = 32;
const STARTUP_INDEX_STATUS_MAX_CHARS: usize = 52;

pub(super) fn startup_index_setup_panel(
    setup: &StartupIndexSetupState,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    let build_button = if setup.can_accept() {
        primary_action_button("Start indexing", Message::StartupIndexAccepted)
    } else {
        inactive_primary_action_button("Start indexing")
    };

    let actions = row![Space::new().width(Length::Fill), build_button]
        .spacing(6)
        .align_y(Alignment::Center);

    let mut content = column![
        readable_text("Build a search index?").size(16),
        startup_index_target_choices(setup),
    ]
    .spacing(12)
    .width(Length::Fill);

    if setup.target_mode.is_some() {
        content = content.push(startup_index_capability_choices(setup));
    }

    if setup.capability.is_some() {
        match setup.target_mode {
            Some(StartupIndexTargetMode::Common) => {
                content = content.push(startup_index_common_roots_listing(setup));
            }
            Some(StartupIndexTargetMode::Custom) => {
                let scroll_region = ScrollbarRegion::StartupIndexSetup;
                content = content
                    .push(startup_index_hidden_content_toggle(
                        setup.show_hidden_entries,
                    ))
                    .push(
                        scrollable(smooth_scroll_content(
                            startup_index_tree_listing(setup),
                            scroll_region.clone(),
                        ))
                        .id(smooth_scroll_id(&scroll_region))
                        .direction(auto_hide_vertical_scrollbar_direction(
                            scrollbar_visibility,
                            6.0,
                        ))
                        .style(auto_hide_scrollbar_style(scrollbar_visibility))
                        .height(Length::Fixed(STARTUP_INDEX_TREE_HEIGHT))
                        .on_scroll(|_| Message::StartupIndexSetupScrolled),
                    );
            }
            None => {}
        }
    }

    content = content.push(actions);

    container(content)
        .padding(14)
        .width(Length::Fixed(STARTUP_INDEX_PANEL_WIDTH))
        .style(context_menu_style)
        .into()
}

fn startup_index_target_choices(setup: &StartupIndexSetupState) -> Column<'static, Message> {
    column![
        selectable_choice_row(
            "Common locations",
            "Desktop, documents, downloads, media, and user config.",
            setup.target_mode == Some(StartupIndexTargetMode::Common),
            Message::StartupIndexTargetModeSelected(StartupIndexTargetMode::Common),
        ),
        selectable_choice_row(
            "Custom selection",
            "Choose folders or files from Home.",
            setup.target_mode == Some(StartupIndexTargetMode::Custom),
            Message::StartupIndexTargetModeSelected(StartupIndexTargetMode::Custom),
        ),
    ]
    .spacing(6)
}

fn startup_index_capability_choices(setup: &StartupIndexSetupState) -> Column<'static, Message> {
    column![
        selectable_choice_row(
            "Filenames",
            "Filename and path catalog.",
            setup.capability == Some(StartupIndexCapability::Filenames),
            Message::StartupIndexCapabilitySelected(StartupIndexCapability::Filenames),
        ),
        selectable_choice_row(
            "Filenames + images",
            "Filename/path catalog plus image metadata.",
            setup.capability == Some(StartupIndexCapability::ImageMetadata),
            Message::StartupIndexCapabilitySelected(StartupIndexCapability::ImageMetadata),
        ),
    ]
    .spacing(6)
}

fn startup_index_common_roots_listing(setup: &StartupIndexSetupState) -> Column<'static, Message> {
    let mut listing = Column::new().spacing(4).width(Length::Fill);
    if setup.common_roots.is_empty() {
        return listing.push(readable_text("No common locations found").size(13));
    }
    for root in &setup.common_roots {
        listing = listing.push(
            container(
                row![
                    readable_text(&root.label).size(13).width(Length::Fill),
                    readable_text(format_middle_ellipsized_text(
                        &root.path.to_string_lossy(),
                        STARTUP_INDEX_STATUS_MAX_CHARS,
                    ))
                    .size(11),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([2, 4])
            .width(Length::Fill),
        );
    }
    listing
}

fn startup_index_tree_listing(setup: &StartupIndexSetupState) -> Column<'static, Message> {
    let mut listing = Column::new().spacing(1).width(Length::Fill);
    if setup.entries.is_empty() {
        return listing.push(readable_text("No indexable locations found").size(14));
    }

    for entry in visible_startup_index_entries(&setup.entries) {
        listing = listing.push(startup_index_tree_entry_row(entry));
        if let Some(message) = startup_index_directory_status_message(entry) {
            listing = listing.push(startup_index_tree_status_row(entry, message));
        }
    }

    listing
}

fn startup_index_directory_status_message(entry: &StartupIndexTreeEntry) -> Option<String> {
    if !entry.is_expanded {
        return None;
    }

    match entry.directory_children.as_ref()? {
        StartupIndexDirectoryChildren::Loading => Some("Loading...".to_owned()),
        StartupIndexDirectoryChildren::Error(error) => Some(format!("Could not load: {error}")),
        StartupIndexDirectoryChildren::Pending | StartupIndexDirectoryChildren::Loaded => None,
    }
}

fn visible_startup_index_entries(entries: &[StartupIndexTreeEntry]) -> Vec<&StartupIndexTreeEntry> {
    entries
        .iter()
        .filter(|entry| startup_index_entry_visible(entry, entries))
        .collect()
}

fn startup_index_entry_visible(
    entry: &StartupIndexTreeEntry,
    entries: &[StartupIndexTreeEntry],
) -> bool {
    let mut parent = entry.parent;
    while let Some(parent_id) = parent {
        let Some(parent_entry) = entries.get(parent_id) else {
            return false;
        };
        if !(parent_entry.is_expanded || parent_entry.toggle_rotation_progress > 0.0) {
            return false;
        }
        parent = parent_entry.parent;
    }

    true
}

fn startup_index_tree_entry_row(entry: &StartupIndexTreeEntry) -> Element<'static, Message> {
    let name = format_middle_ellipsized_text(&entry.name, STARTUP_INDEX_NAME_MAX_CHARS);
    let indent = Space::new().width(Length::Fixed(
        entry.depth as f32 * STARTUP_INDEX_TREE_INDENT_WIDTH,
    ));
    let toggle = startup_index_directory_toggle(entry);
    let entry_id = entry.id;
    let selector = checkbox(entry.selection.is_selected());
    let label = container(
        row![
            themed_icon(
                preview_entry_icon_symbol(entry.kind, &entry.name),
                IconTone::Normal,
                STARTUP_INDEX_ICON_SIZE,
            ),
            readable_text(name).size(14).width(Length::Fill),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill);

    let row_content = row![indent, toggle, selector, label]
        .spacing(4)
        .align_y(Alignment::Center);

    mouse_area(container(row_content).padding([1, 4]).width(Length::Fill))
        .on_press(Message::StartupIndexEntryPressed(entry_id))
        .on_enter(Message::StartupIndexEntryEntered(entry_id))
        .on_release(Message::StartupIndexEntrySelectionDragFinished)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

fn startup_index_directory_toggle(entry: &StartupIndexTreeEntry) -> Element<'static, Message> {
    if !entry.is_directory() {
        return Space::new()
            .width(Length::Fixed(STARTUP_INDEX_TREE_TOGGLE_WIDTH))
            .into();
    }

    let rotation = entry.toggle_rotation_progress * STARTUP_INDEX_TREE_TOGGLE_ROTATION_DEGREES;
    let toggle = container(
        rotated_chevron_right_view(rotation, STARTUP_INDEX_TREE_TOGGLE_WIDTH)
            .style(icon_tone_style(IconTone::Normal)),
    )
    .width(Length::Fixed(STARTUP_INDEX_TREE_TOGGLE_WIDTH))
    .height(Length::Fixed(STARTUP_INDEX_TREE_TOGGLE_WIDTH))
    .center_x(Length::Fixed(STARTUP_INDEX_TREE_TOGGLE_WIDTH))
    .center_y(Length::Fixed(STARTUP_INDEX_TREE_TOGGLE_WIDTH));

    mouse_area(toggle)
        .on_press(Message::StartupIndexDirectoryToggled(entry.id))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

fn startup_index_tree_status_row(
    entry: &StartupIndexTreeEntry,
    message: String,
) -> Element<'static, Message> {
    let message = format_middle_ellipsized_text(&message, STARTUP_INDEX_STATUS_MAX_CHARS);
    let indent = Space::new().width(Length::Fixed(
        (entry.depth + 1) as f32 * STARTUP_INDEX_TREE_INDENT_WIDTH,
    ));
    let row_content = row![
        indent,
        Space::new().width(Length::Fixed(STARTUP_INDEX_TREE_TOGGLE_WIDTH)),
        Space::new().width(Length::Fixed(STARTUP_INDEX_ICON_SIZE)),
        readable_text(message).size(13).width(Length::Fill),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    container(row_content)
        .padding([1, 4])
        .width(Length::Fill)
        .into()
}

fn startup_index_hidden_content_toggle(show_hidden_entries: bool) -> Element<'static, Message> {
    let content = row![
        readable_text("Show hidden content").size(12),
        switch_control(show_hidden_entries),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    mouse_area(content)
        .on_press(Message::StartupIndexHiddenContentVisibilityToggled)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}
