use file_core::{DirectoryEntry, FileKind};
use iced::widget::{container, mouse_area, row, scrollable, text_input, Column, Space};
use iced::{Alignment, Element, Length};

use crate::app::panes::BrowserPaneView;
use crate::app::FileBrowser;
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, icon_svg_style,
    list_header_style, list_panel_style, list_row_style,
};
use crate::column_entry_bounds::track_column_entry_bounds;
use crate::file_entry_presentation::selection_run_position;
use crate::file_entry_view::{entry_thumbnail_or_icon, FileEntryIconTone, FileEntryVisualState};
use crate::formatting::{format_file_size, format_system_time};
use crate::icons::rotated_chevron_right_view;
use crate::measured_middle_ellipsized_text::measured_middle_ellipsized_text;
use crate::model::{ExpandedDirectoryStatus, Message};
use crate::typography::readable_text;
use crate::view::rename_input_id;

const LIST_ROW_TEXT_SIZE: u32 = 15;
const LIST_HEADER_TEXT_SIZE: u32 = 12;
const LIST_CONTENT_PADDING: [u16; 2] = [4, 6];
const LIST_ROW_HEIGHT: f32 = 46.0;
const LIST_ROW_PADDING: [u16; 2] = [0, 8];
const LIST_HEADER_PADDING: [u16; 2] = [4, 8];
const LIST_ROW_SPACING: u32 = 6;
const LIST_INDENT_WIDTH: f32 = 18.0;
const LIST_TOGGLE_WIDTH: f32 = 18.0;
const LIST_TOGGLE_ICON_SIZE: f32 = 14.0;
const LIST_NAME_PORTION: u16 = 54;
const LIST_MODIFIED_PORTION: u16 = 22;
const LIST_SIZE_PORTION: u16 = 12;
const LIST_KIND_PORTION: u16 = 12;

pub(crate) fn list_browser_view<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
) -> Element<'a, Message> {
    let mut rows = Column::new().spacing(0).width(Length::Fill);
    rows = rows.push(list_header());

    if pane.is_loading && pane.entries.is_empty() {
        rows = rows.push(list_message("Loading..."));
    } else if pane.entries.is_empty() {
        rows = rows.push(list_message(if pane.is_trash_view {
            "Trash is empty"
        } else {
            "No items"
        }));
    } else {
        let visible_entries =
            crate::visible_entries::visible_entries(pane.entries, pane.expanded_directories);
        let visible_paths = visible_entries
            .iter()
            .map(|visible_entry| visible_entry.entry.path.clone())
            .collect::<Vec<_>>();
        for (row_index, visible_entry) in visible_entries.iter().enumerate() {
            rows = rows.push(list_entry_row(
                browser,
                pane,
                visible_entry.entry,
                visible_entry.depth,
                row_index,
                visible_entry.animation_progress,
                selection_run_position(&visible_paths, pane.selected_paths, row_index),
            ));
            if let Some(status_row) = list_directory_status_for_entry(
                pane,
                visible_entry.entry,
                visible_entry.depth + 1,
                row_index,
            ) {
                rows = rows.push(status_row);
            }
        }
    }

    let list_scroll = scrollable(rows)
        .direction(auto_hide_vertical_scrollbar_direction(
            browser.scrollbar_visibility,
            8.0,
        ))
        .style(auto_hide_scrollbar_style(browser.scrollbar_visibility))
        .width(Length::Fill)
        .height(Length::Fill)
        .on_scroll(move |viewport| {
            let offset = viewport.absolute_offset();
            let bounds = viewport.bounds();
            Message::ListScrolled(pane.id, offset.y, bounds.height)
        });

    mouse_area(
        container(list_scroll)
            .padding(LIST_CONTENT_PADDING)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(list_panel_style),
    )
    .on_press(Message::BlankAreaPressed(pane.id))
    .on_release(Message::DropTargetReleased(
        pane.id,
        pane.current_dir.clone(),
    ))
    .on_right_press(Message::BlankAreaRightClicked(
        pane.id,
        pane.current_dir.clone(),
    ))
    .on_enter(Message::ColumnBrowserCursorEntered(pane.id))
    .on_exit(Message::ColumnBrowserCursorExited(pane.id))
    .into()
}

fn list_directory_status_for_entry<'a>(
    pane: BrowserPaneView<'a>,
    entry: &DirectoryEntry,
    depth: usize,
    parent_row_index: usize,
) -> Option<Element<'static, Message>> {
    let expanded = pane
        .expanded_directories
        .get(&entry.path)
        .filter(|expanded| expanded.is_expanded || expanded.is_collapsing)?;

    let animation_height = LIST_ROW_HEIGHT * expanded.animation_progress.clamp(0.0, 1.0);
    match &expanded.status {
        ExpandedDirectoryStatus::Loading => Some(list_directory_status_row(
            "Loading...",
            depth,
            parent_row_index,
            animation_height,
        )),
        ExpandedDirectoryStatus::Error => Some(list_directory_status_row(
            "Could not load",
            depth,
            parent_row_index,
            animation_height,
        )),
        ExpandedDirectoryStatus::Loaded if expanded.entries.is_empty() => Some(
            list_directory_status_row("No items", depth, parent_row_index, animation_height),
        ),
        ExpandedDirectoryStatus::Loaded => None,
    }
}

fn list_header() -> Element<'static, Message> {
    let header = row![
        header_cell("Name", LIST_NAME_PORTION),
        header_cell("Date Modified", LIST_MODIFIED_PORTION),
        header_cell("Size", LIST_SIZE_PORTION),
        header_cell("Kind", LIST_KIND_PORTION),
    ]
    .spacing(0)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    container(header)
        .padding(LIST_HEADER_PADDING)
        .width(Length::Fill)
        .style(list_header_style)
        .into()
}

fn header_cell(label: &'static str, portion: u16) -> Element<'static, Message> {
    container(readable_text(label).size(LIST_HEADER_TEXT_SIZE))
        .width(Length::FillPortion(portion))
        .into()
}

fn list_message(message: &'static str) -> Element<'static, Message> {
    container(readable_text(message).size(LIST_ROW_TEXT_SIZE))
        .padding(LIST_ROW_PADDING)
        .width(Length::Fill)
        .into()
}

fn list_directory_status_row(
    message: &'static str,
    depth: usize,
    row_index: usize,
    height: f32,
) -> Element<'static, Message> {
    let indent_width = depth as f32 * LIST_INDENT_WIDTH + LIST_TOGGLE_WIDTH + 26.0;
    container(
        container(row![
            Space::new().width(Length::Fixed(indent_width)),
            readable_text(message).size(LIST_ROW_TEXT_SIZE),
        ])
        .height(Length::Fixed(LIST_ROW_HEIGHT))
        .center_y(Length::Fixed(LIST_ROW_HEIGHT))
        .padding(LIST_ROW_PADDING)
        .width(Length::Fill)
        .style(list_row_style(depth, row_index)),
    )
    .height(Length::Fixed(height))
    .clip(true)
    .into()
}

fn list_entry_row<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
    entry: &DirectoryEntry,
    depth: usize,
    row_index: usize,
    animation_progress: f32,
    selection_run_position: Option<crate::file_entry_presentation::SelectionRunPosition>,
) -> Element<'a, Message> {
    let visual_state = FileEntryVisualState::from_entry_context(pane, &entry.path, false);
    let icon_tone = visual_state.icon_tone();
    let name_cell = list_name_cell(browser, pane, entry, depth, icon_tone);
    let row_content = row![
        name_cell,
        text_cell(modified_text(entry), LIST_MODIFIED_PORTION),
        text_cell(size_text(entry), LIST_SIZE_PORTION),
        text_cell(kind_text(entry), LIST_KIND_PORTION),
    ]
    .spacing(0)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let row_container = container(row_content)
        .padding(LIST_ROW_PADDING)
        .height(Length::Fixed(LIST_ROW_HEIGHT))
        .center_y(Length::Fixed(LIST_ROW_HEIGHT))
        .width(Length::Fill);
    let row_container =
        if let Some(style) = visual_state.row_style_for_selection_run(selection_run_position) {
            row_container.style(style)
        } else {
            row_container.style(list_row_style(depth, row_index))
        };

    let row_area = mouse_area(row_container)
        .on_enter(Message::EntryHovered(pane.id, entry.path.clone()))
        .on_exit(Message::EntryHoverCleared(pane.id, entry.path.clone()))
        .on_press(Message::ListEntryClicked(pane.id, entry.path.clone()))
        .on_release(Message::EntryReleased(pane.id, entry.path.clone()))
        .on_right_press(Message::EntryRightClicked(pane.id, entry.path.clone()))
        .interaction(iced::mouse::Interaction::Pointer);

    let row_area = if entry.kind == FileKind::Directory && !pane.is_trash_view {
        row_area.on_middle_press(Message::OpenDirectoryFromMiddleClick(
            pane.id,
            entry.path.clone(),
        ))
    } else {
        row_area
    };

    let animated_row = container(row_area)
        .height(Length::Fixed(
            LIST_ROW_HEIGHT * animation_progress.clamp(0.0, 1.0),
        ))
        .clip(true);
    track_column_entry_bounds(animated_row, pane.id, entry.path.clone())
}

fn list_name_cell<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
    entry: &DirectoryEntry,
    depth: usize,
    icon_tone: FileEntryIconTone,
) -> Element<'a, Message> {
    let indent = Space::new().width(Length::Fixed(depth as f32 * LIST_INDENT_WIDTH));
    let toggle = list_directory_toggle(pane, entry);
    let name: Element<'a, Message> = if pane.renaming == Some(&entry.path) {
        text_input("File name", pane.rename_input)
            .id(rename_input_id())
            .on_input(Message::RenameInputChanged)
            .on_submit(Message::RenameSelected)
            .width(Length::Fill)
            .into()
    } else {
        measured_middle_ellipsized_text(
            entry.name().to_string_lossy().into_owned(),
            LIST_ROW_TEXT_SIZE,
        )
    };

    row![
        indent,
        toggle,
        entry_thumbnail_or_icon(browser, entry, icon_tone),
        name
    ]
    .spacing(LIST_ROW_SPACING)
    .align_y(Alignment::Center)
    .width(Length::FillPortion(LIST_NAME_PORTION))
    .into()
}

fn list_directory_toggle<'a>(
    pane: BrowserPaneView<'a>,
    entry: &DirectoryEntry,
) -> Element<'a, Message> {
    if entry.kind != FileKind::Directory || pane.is_trash_view {
        return Space::new().width(Length::Fixed(LIST_TOGGLE_WIDTH)).into();
    }

    let rotation = pane
        .expanded_directories
        .get(&entry.path)
        .filter(|expanded| expanded.is_expanded)
        .map(|expanded| 90.0 * expanded.animation_progress.clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let icon = rotated_chevron_right_view(rotation, LIST_TOGGLE_ICON_SIZE).style(icon_svg_style());
    mouse_area(
        container(icon)
            .center_x(Length::Fixed(LIST_TOGGLE_WIDTH))
            .center_y(Length::Fixed(LIST_TOGGLE_WIDTH)),
    )
    .on_press(Message::ListDirectoryToggled(pane.id, entry.path.clone()))
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

fn text_cell(text: String, portion: u16) -> Element<'static, Message> {
    container(readable_text(text).size(LIST_ROW_TEXT_SIZE))
        .width(Length::FillPortion(portion))
        .into()
}

fn modified_text(entry: &DirectoryEntry) -> String {
    entry
        .metadata
        .modified
        .map(format_system_time)
        .unwrap_or_else(|| "-".to_owned())
}

fn size_text(entry: &DirectoryEntry) -> String {
    if entry.kind == FileKind::Directory {
        "-".to_owned()
    } else {
        format_file_size(entry.metadata.len)
    }
}

fn kind_text(entry: &DirectoryEntry) -> String {
    let kind = match entry.kind {
        FileKind::Directory => "Folder",
        FileKind::File => "File",
        FileKind::Symlink if entry.is_broken_symlink => "Broken Link",
        FileKind::Symlink => "Link",
        FileKind::Other => "Other",
    };
    kind.to_owned()
}
