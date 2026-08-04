use file_core::{DirectoryEntry, FileKind, SortDirection};
use iced::widget::{container, mouse_area, row, scrollable, text_input, Column, Row, Space, Stack};
use iced::{Alignment, Element, Length};

use crate::app::panes::{BrowserPaneView, DirectoryContentAvailability};
use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use crate::app::FileBrowser;
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, icon_svg_style,
    list_header_cell_style, list_header_reorder_indicator_style, list_header_style,
    list_panel_style, list_row_style, ListHeaderCellVisualState,
};
use crate::column_entry_bounds::track_column_entry_bounds;
use crate::file_entry_presentation::SelectionRunPosition;
use crate::file_entry_view::{
    entry_text_input_style, entry_thumbnail_or_icon, FileEntryIconDensity, FileEntryIconTone,
    FileEntryVisualState,
};
use crate::formatting::format_system_time;
use crate::icons::rotated_chevron_right_view;
use crate::measured_middle_ellipsized_text::measured_middle_ellipsized_text;
use crate::model::{
    BrowserPaneId, DirectoryLoadingPlaceholderEntry, ExpandedDirectoryStatus,
    FilePropertiesPermissions, ListColumnConfig, ListColumnKind, Message, ScrollbarRegion,
};
use crate::typography::readable_text;
use crate::view::rename_input_id;
use crate::virtual_range::{initial_virtual_range, virtual_range_for_viewport};

const LIST_ROW_TEXT_SIZE: u32 = 15;
const LIST_HEADER_TEXT_SIZE: u32 = 12;
const LIST_HEADER_SORT_ICON_SIZE: f32 = 12.0;
const LIST_CONTENT_PADDING: iced::Padding = iced::Padding {
    top: 0.0,
    right: 6.0,
    bottom: 4.0,
    left: 6.0,
};
pub(crate) const LIST_ROW_HEIGHT: f32 = 46.0;
pub(crate) const LIST_OVERSCAN_ROWS: usize = 16;
pub(crate) const LIST_INITIAL_ROWS: usize = LIST_OVERSCAN_ROWS * 2 + 1;
const LIST_ROW_PADDING: [u16; 2] = [0, 8];
const LIST_HEADER_PADDING: [u16; 2] = [4, 8];
const LIST_ROW_SPACING: u32 = 6;
const LIST_INDENT_WIDTH: f32 = 18.0;
const LIST_TOGGLE_WIDTH: f32 = 18.0;
const LIST_TOGGLE_ICON_SIZE: f32 = 14.0;
const LIST_HEADER_CELL_HEIGHT: f32 = 24.0;
const LIST_HEADER_DROP_INDICATOR_WIDTH: f32 = 3.0;
const LIST_COLUMN_RESIZE_DIVIDER_WIDTH: f32 = 5.0;

pub(crate) fn list_browser_view<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
) -> Element<'a, Message> {
    let mut rows = Column::new().spacing(0).width(Length::Fill);
    rows = rows.push(list_header(browser, pane.id));

    if matches!(
        pane.current_directory_content(),
        DirectoryContentAvailability::Pending
    ) {
        for (row_index, placeholder) in pane
            .directory_loading_placeholder_entries
            .iter()
            .enumerate()
        {
            rows = rows.push(list_placeholder_entry_row(browser, placeholder, row_index));
        }
    } else if pane.entries.is_empty() {
        rows = rows.push(list_message(if pane.is_trash_view {
            "Trash is empty"
        } else {
            "No items"
        }));
    } else {
        let total_rows =
            crate::visible_entries::visible_entry_count(pane.entries, pane.expanded_directories);
        let range = pane
            .column_viewports
            .get(pane.current_dir)
            .map(|viewport| {
                virtual_range_for_viewport(
                    total_rows,
                    LIST_ROW_HEIGHT,
                    viewport.offset_y,
                    viewport.height,
                    LIST_OVERSCAN_ROWS,
                )
            })
            .unwrap_or_else(|| {
                initial_virtual_range(total_rows, LIST_ROW_HEIGHT, LIST_INITIAL_ROWS)
            });
        rows = rows.push(vertical_spacer(range.before_height));

        let entries_with_neighbors = crate::visible_entries::visible_entries_in_range(
            pane.entries,
            pane.expanded_directories,
            range.start.saturating_sub(1),
            range.end.saturating_add(1),
        );
        let neighbor_offset = usize::from(range.start > 0);
        let rendered_count = range.end.saturating_sub(range.start);
        for local_index in 0..rendered_count {
            let entry_index = local_index + neighbor_offset;
            let Some(visible_entry) = entries_with_neighbors.get(entry_index) else {
                break;
            };
            let row_index = range.start + local_index;
            rows = rows.push(list_entry_row(
                browser,
                pane,
                visible_entry.entry,
                visible_entry.depth,
                row_index,
                visible_entry.animation_progress,
                selection_run_position_for_visible_neighbors(
                    &entries_with_neighbors,
                    entry_index,
                    pane.selected_paths,
                ),
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
        rows = rows.push(vertical_spacer(range.after_height));
    }

    let scrollbar_region = ScrollbarRegion::PaneList(pane.id);
    let scrollbar_visibility = browser.scrollbar_visibility_for(&scrollbar_region);
    let list_scroll = scrollable(smooth_scroll_content(rows, scrollbar_region.clone()))
        .id(smooth_scroll_id(&scrollbar_region))
        .direction(auto_hide_vertical_scrollbar_direction(
            scrollbar_visibility,
            8.0,
        ))
        .style(auto_hide_scrollbar_style(scrollbar_visibility))
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

fn selection_run_position_for_visible_neighbors(
    entries: &[crate::visible_entries::VisibleEntry<'_>],
    index: usize,
    selected_paths: &std::collections::HashSet<std::path::PathBuf>,
) -> Option<SelectionRunPosition> {
    let entry = entries.get(index)?;
    if !selected_paths.contains(&entry.entry.path) {
        return None;
    }
    let previous_selected = index
        .checked_sub(1)
        .and_then(|previous| entries.get(previous))
        .is_some_and(|previous| selected_paths.contains(&previous.entry.path));
    let next_selected = entries
        .get(index + 1)
        .is_some_and(|next| selected_paths.contains(&next.entry.path));
    Some(SelectionRunPosition::from_neighbors(
        previous_selected,
        next_selected,
    ))
}

fn vertical_spacer(height: f32) -> Element<'static, Message> {
    Space::new().height(Length::Fixed(height.max(0.0))).into()
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
        ExpandedDirectoryStatus::Loading => None,
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

fn list_header<'a>(browser: &'a FileBrowser, pane_id: BrowserPaneId) -> Element<'a, Message> {
    let visible_columns = browser
        .user_config()
        .list_view_preferences
        .visible_columns()
        .collect::<Vec<_>>();
    let hovered_column = browser.hovered_list_header_column(pane_id);
    let dragged_column = browser.list_column_being_reordered();
    let drop_target = browser.list_column_reorder_insertion_target();
    let sort = browser.user_config().list_view_preferences.sort();
    let mut header = Row::new()
        .spacing(0)
        .align_y(Alignment::Center)
        .width(Length::Fill);
    for (index, column) in visible_columns.iter().enumerate() {
        if let Some(previous) = index
            .checked_sub(1)
            .and_then(|previous| visible_columns.get(previous))
        {
            header = header.push(list_column_resize_divider(pane_id, previous.kind));
        }
        let sort_direction = column
            .kind
            .sort_field()
            .filter(|field| *field == sort.field)
            .map(|_| sort.direction);
        let visual_state = if drop_target == Some(column.kind) {
            ListHeaderCellVisualState::DropTarget
        } else if dragged_column == Some(column.kind) {
            ListHeaderCellVisualState::Dragged
        } else if hovered_column == Some(column.kind) {
            ListHeaderCellVisualState::Hovered
        } else {
            ListHeaderCellVisualState::Idle
        };
        header = header.push(header_cell(pane_id, column, sort_direction, visual_state));
    }
    if let Some(last_column) = visible_columns.last() {
        header = header.push(list_column_resize_divider(pane_id, last_column.kind));
    }

    mouse_area(
        container(header)
            .padding(LIST_HEADER_PADDING)
            .width(Length::Fill)
            .style(list_header_style),
    )
    .on_right_press(Message::ListHeaderRightClicked(pane_id))
    .into()
}

fn header_cell(
    pane_id: BrowserPaneId,
    column: &ListColumnConfig,
    sort_direction: Option<SortDirection>,
    visual_state: ListHeaderCellVisualState,
) -> Element<'static, Message> {
    let content: Element<'static, Message> = if let Some(direction) = sort_direction {
        row![
            readable_text(column.kind.label()).size(LIST_HEADER_TEXT_SIZE),
            list_sort_direction_indicator(direction),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    } else {
        readable_text(column.kind.label())
            .size(LIST_HEADER_TEXT_SIZE)
            .into()
    };
    let mut layers = Stack::new()
        .width(list_column_width(column.width))
        .height(Length::Fixed(LIST_HEADER_CELL_HEIGHT));
    layers = layers.push(
        container(content)
            .padding([0, 6])
            .center_y(Length::Fill)
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .style(list_header_cell_style(visual_state)),
    );
    if visual_state == ListHeaderCellVisualState::DropTarget {
        layers = layers.push(list_header_drop_indicator());
    }

    mouse_area(
        container(layers)
            .center_y(Length::Fixed(LIST_HEADER_CELL_HEIGHT))
            .width(list_column_width(column.width))
            .height(Length::Fixed(LIST_HEADER_CELL_HEIGHT)),
    )
    .on_press(Message::ListColumnReorderStarted(pane_id, column.kind))
    .on_enter(Message::ListHeaderColumnEntered(pane_id, column.kind))
    .on_exit(Message::ListHeaderColumnExited(pane_id, column.kind))
    .on_release(Message::DragSelectionFinished)
    .interaction(iced::mouse::Interaction::Grab)
    .into()
}

fn list_sort_direction_indicator(direction: SortDirection) -> Element<'static, Message> {
    let rotation = match direction {
        SortDirection::Ascending => -90.0,
        SortDirection::Descending => 90.0,
    };
    rotated_chevron_right_view(rotation, LIST_HEADER_SORT_ICON_SIZE)
        .style(icon_svg_style())
        .into()
}

fn list_header_drop_indicator() -> Element<'static, Message> {
    row![
        container(Space::new())
            .width(Length::Fixed(LIST_HEADER_DROP_INDICATOR_WIDTH))
            .height(Length::Fill)
            .style(list_header_reorder_indicator_style),
        Space::new().width(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn list_column_resize_divider(
    pane_id: BrowserPaneId,
    column: ListColumnKind,
) -> Element<'static, Message> {
    let divider = Space::new()
        .width(Length::Fixed(LIST_COLUMN_RESIZE_DIVIDER_WIDTH))
        .height(Length::Fill);

    mouse_area(divider)
        .on_press(Message::ListColumnResizeStarted(pane_id, column))
        .on_release(Message::DragSelectionFinished)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
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
    let modifier = browser.file_entry_content_modifier(&entry.path);
    let icon_tone = visual_state.icon_tone();
    let row_content = container(list_entry_cells(browser, pane, entry, depth, icon_tone))
        .width(Length::Fill)
        .style(visual_state.content_style(modifier));

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
        .on_press(Message::FlatEntryClicked(pane.id, entry.path.clone()))
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

fn list_placeholder_entry_row<'a>(
    browser: &'a FileBrowser,
    placeholder: &'a DirectoryLoadingPlaceholderEntry,
    row_index: usize,
) -> Element<'a, Message> {
    let entry = &placeholder.entry;
    let modifier = browser.file_entry_content_modifier(&entry.path);
    let row_content = container(list_placeholder_entry_cells(
        browser,
        entry,
        placeholder.depth,
    ))
    .width(Length::Fill)
    .style(FileEntryVisualState::Normal.content_style(modifier));

    container(
        container(row_content)
            .padding(LIST_ROW_PADDING)
            .height(Length::Fixed(LIST_ROW_HEIGHT))
            .center_y(Length::Fixed(LIST_ROW_HEIGHT))
            .width(Length::Fill)
            .style(list_row_style(placeholder.depth, row_index)),
    )
    .height(Length::Fixed(
        LIST_ROW_HEIGHT * placeholder.animation_progress.clamp(0.0, 1.0),
    ))
    .clip(true)
    .into()
}

fn list_entry_cells<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
    entry: &DirectoryEntry,
    depth: usize,
    icon_tone: FileEntryIconTone,
) -> Row<'a, Message> {
    let visible_columns = browser
        .user_config()
        .list_view_preferences
        .visible_columns()
        .collect::<Vec<_>>();
    let mut row_content = Row::new()
        .spacing(0)
        .align_y(Alignment::Center)
        .width(Length::Fill);
    for (index, column) in visible_columns.iter().enumerate() {
        if index > 0 {
            row_content = row_content.push(list_column_gap());
        }
        row_content = row_content.push(list_entry_cell(
            browser, pane, entry, depth, icon_tone, column,
        ));
    }
    row_content.push(list_column_gap())
}

fn list_placeholder_entry_cells<'a>(
    browser: &'a FileBrowser,
    entry: &DirectoryEntry,
    depth: usize,
) -> Row<'a, Message> {
    let visible_columns = browser
        .user_config()
        .list_view_preferences
        .visible_columns()
        .collect::<Vec<_>>();
    let mut row_content = Row::new()
        .spacing(0)
        .align_y(Alignment::Center)
        .width(Length::Fill);
    for (index, column) in visible_columns.iter().enumerate() {
        if index > 0 {
            row_content = row_content.push(list_column_gap());
        }
        row_content = row_content.push(list_placeholder_entry_cell(browser, entry, depth, column));
    }
    row_content.push(list_column_gap())
}

fn list_entry_cell<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
    entry: &DirectoryEntry,
    depth: usize,
    icon_tone: FileEntryIconTone,
    column: &ListColumnConfig,
) -> Element<'a, Message> {
    match column.kind {
        ListColumnKind::Name => {
            list_name_cell(browser, pane, entry, depth, icon_tone, column.width)
        }
        ListColumnKind::Modified => text_cell(modified_text(entry), column.width),
        ListColumnKind::Size => text_cell(browser.list_directory_size_text(entry), column.width),
        ListColumnKind::Kind => text_cell(kind_text(entry), column.width),
        ListColumnKind::Owner => text_cell(owner_text(entry), column.width),
        ListColumnKind::Group => text_cell(group_text(entry), column.width),
        ListColumnKind::Permissions => text_cell(permissions_text(entry), column.width),
        ListColumnKind::Accessed => text_cell(accessed_text(entry), column.width),
        ListColumnKind::Created => text_cell(created_text(entry), column.width),
    }
}

fn list_placeholder_entry_cell<'a>(
    browser: &'a FileBrowser,
    entry: &DirectoryEntry,
    depth: usize,
    column: &ListColumnConfig,
) -> Element<'a, Message> {
    match column.kind {
        ListColumnKind::Name => list_placeholder_name_cell(browser, entry, depth, column.width),
        ListColumnKind::Modified => text_cell(modified_text(entry), column.width),
        ListColumnKind::Size => text_cell(browser.list_directory_size_text(entry), column.width),
        ListColumnKind::Kind => text_cell(kind_text(entry), column.width),
        ListColumnKind::Owner => text_cell(owner_text(entry), column.width),
        ListColumnKind::Group => text_cell(group_text(entry), column.width),
        ListColumnKind::Permissions => text_cell(permissions_text(entry), column.width),
        ListColumnKind::Accessed => text_cell(accessed_text(entry), column.width),
        ListColumnKind::Created => text_cell(created_text(entry), column.width),
    }
}

fn list_column_gap() -> Element<'static, Message> {
    Space::new()
        .width(Length::Fixed(LIST_COLUMN_RESIZE_DIVIDER_WIDTH))
        .into()
}

fn list_column_width(width: f32) -> Length {
    Length::FillPortion(width.round().clamp(1.0, u16::MAX as f32) as u16)
}

fn list_placeholder_name_cell<'a>(
    browser: &'a FileBrowser,
    entry: &DirectoryEntry,
    depth: usize,
    width: f32,
) -> Element<'a, Message> {
    row![
        Space::new().width(Length::Fixed(depth as f32 * LIST_INDENT_WIDTH)),
        Space::new().width(Length::Fixed(LIST_TOGGLE_WIDTH)),
        entry_thumbnail_or_icon(
            browser,
            entry,
            FileEntryIconTone::Normal,
            FileEntryIconDensity::List,
        ),
        measured_middle_ellipsized_text(
            entry.name().to_string_lossy().into_owned(),
            LIST_ROW_TEXT_SIZE,
        )
    ]
    .spacing(LIST_ROW_SPACING)
    .align_y(Alignment::Center)
    .width(list_column_width(width))
    .into()
}

fn list_name_cell<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
    entry: &DirectoryEntry,
    depth: usize,
    icon_tone: FileEntryIconTone,
    width: f32,
) -> Element<'a, Message> {
    let modifier = browser.file_entry_content_modifier(&entry.path);
    let indent = Space::new().width(Length::Fixed(depth as f32 * LIST_INDENT_WIDTH));
    let toggle = list_directory_toggle(pane, entry);
    let name: Element<'a, Message> = if pane.renaming == Some(&entry.path) {
        text_input(
            &crate::localization::translate_current("File name"),
            pane.rename_input,
        )
        .id(rename_input_id())
        .on_input(Message::RenameInputChanged)
        .on_submit(Message::RenameSelected)
        .style(entry_text_input_style(modifier))
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
        entry_thumbnail_or_icon(browser, entry, icon_tone, FileEntryIconDensity::List),
        name
    ]
    .spacing(LIST_ROW_SPACING)
    .align_y(Alignment::Center)
    .width(list_column_width(width))
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

fn text_cell(text: String, width: f32) -> Element<'static, Message> {
    container(readable_text(text).size(LIST_ROW_TEXT_SIZE))
        .width(list_column_width(width))
        .clip(true)
        .into()
}

fn modified_text(entry: &DirectoryEntry) -> String {
    timestamp_text(entry.metadata.modified)
}

fn kind_text(entry: &DirectoryEntry) -> String {
    let kind = match entry.kind {
        FileKind::Directory => "Folder",
        FileKind::File => "File",
        FileKind::Symlink if entry.is_broken_symlink => "Broken Link",
        FileKind::Symlink => "Link",
        FileKind::Other => "Other",
    };
    crate::localization::translate_current(kind)
}

fn owner_text(entry: &DirectoryEntry) -> String {
    entry
        .metadata
        .owner_name
        .clone()
        .unwrap_or_else(|| "-".to_owned())
}

fn group_text(entry: &DirectoryEntry) -> String {
    entry
        .metadata
        .group_name
        .clone()
        .unwrap_or_else(|| "-".to_owned())
}

fn permissions_text(entry: &DirectoryEntry) -> String {
    entry
        .metadata
        .permissions_mode
        .map(FilePropertiesPermissions::from_mode)
        .map(|permissions| {
            format!(
                "{} ({})",
                permissions.symbolic_string(),
                permissions.octal_string()
            )
        })
        .unwrap_or_else(|| "-".to_owned())
}

fn accessed_text(entry: &DirectoryEntry) -> String {
    timestamp_text(entry.metadata.accessed)
}

fn created_text(entry: &DirectoryEntry) -> String {
    timestamp_text(entry.metadata.created)
}

fn timestamp_text(value: Option<std::time::SystemTime>) -> String {
    value
        .map(format_system_time)
        .unwrap_or_else(|| "-".to_owned())
}

#[cfg(test)]
#[path = "list_view/directory_status_tests.rs"]
mod directory_status_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use file_core::{DirectoryEntry, EntryMetadata, FileKind};
    use std::path::PathBuf;

    #[test]
    fn list_column_width_uses_proportional_layout_weight() {
        assert_eq!(list_column_width(160.0), Length::FillPortion(160));
        assert_eq!(list_column_width(0.0), Length::FillPortion(1));
    }

    #[test]
    fn list_header_spacing_balances_vertical_whitespace() {
        assert_eq!(
            LIST_CONTENT_PADDING,
            iced::Padding {
                top: 0.0,
                right: 6.0,
                bottom: 4.0,
                left: 6.0,
            }
        );
        assert_eq!(LIST_HEADER_PADDING[0], 4);
    }

    fn test_entry(metadata: EntryMetadata) -> DirectoryEntry {
        DirectoryEntry::new(
            PathBuf::from("/workspace/report.txt"),
            FileKind::File,
            metadata,
            false,
            false,
            false,
        )
    }

    #[test]
    fn new_metadata_columns_render_placeholder_when_metadata_is_missing() {
        let entry = test_entry(EntryMetadata::default());

        assert_eq!(owner_text(&entry), "-");
        assert_eq!(group_text(&entry), "-");
        assert_eq!(permissions_text(&entry), "-");
        assert_eq!(accessed_text(&entry), "-");
        assert_eq!(created_text(&entry), "-");
    }

    #[test]
    fn permissions_column_formats_symbolic_and_octal_mode() {
        let entry = test_entry(EntryMetadata {
            permissions_mode: Some(0o755),
            ..EntryMetadata::default()
        });

        assert_eq!(permissions_text(&entry), "rwxr-xr-x (0755)");
    }
}
