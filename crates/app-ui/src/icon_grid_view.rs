use file_core::{DirectoryEntry, FileKind};
use iced::widget::{container, mouse_area, scrollable, text, text_input, Column, Row, Space};
use iced::{alignment, Alignment, Element, Length};

use crate::app::panes::{BrowserPaneView, DirectoryContentAvailability};
use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use crate::app::FileBrowser;
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, list_panel_style,
};
use crate::column_entry_bounds::track_column_entry_bounds;
use crate::file_entry_view::{entry_thumbnail_or_icon, FileEntryIconDensity, FileEntryVisualState};
use crate::icon_grid_geometry::{
    column_count_for_width, row_height, tile_visual_height, tile_width, visible_entry_range,
    ICON_GRID_CONTENT_PADDING, ICON_GRID_GAP,
};
use crate::model::{Message, ScrollbarRegion};
use crate::typography::readable_text;
use crate::view::rename_input_id;

const ICON_GRID_LABEL_HEIGHT: f32 = 36.0;
const ICON_GRID_LABEL_SIZE: u32 = 14;
const ICON_GRID_TILE_PADDING: [u16; 2] = [4, 8];
const ICON_GRID_ICON_LABEL_SPACING: u32 = 8;

pub(crate) fn icon_grid_view<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
) -> Element<'a, Message> {
    let icon_edge = browser.user_config().icon_grid_size;
    let column_count = column_count_for_width(pane.icon_grid_viewport.width, icon_edge);
    let mut content = Column::new()
        .spacing(0)
        .padding(ICON_GRID_CONTENT_PADDING as u16)
        .width(Length::Fill);

    match pane.current_directory_content() {
        DirectoryContentAvailability::Pending => {}
        DirectoryContentAvailability::Available([]) => {
            content = content.push(grid_message(if pane.is_trash_view {
                "Trash is empty"
            } else {
                "No items"
            }));
        }
        DirectoryContentAvailability::Available(entries) => {
            let range = visible_entry_range(pane.icon_grid_viewport, entries.len(), icon_edge);
            content = content.push(vertical_spacer(range.before_height));
            for row_index in range.start_row..range.end_row {
                let mut row = Row::new()
                    .spacing(ICON_GRID_GAP)
                    .align_y(Alignment::Start)
                    .width(Length::Fill)
                    .height(Length::Fixed(row_height(icon_edge)));
                let start = row_index.saturating_mul(column_count);
                let end = start.saturating_add(column_count).min(entries.len());
                for entry in &entries[start..end] {
                    row = row.push(icon_grid_entry(browser, pane, entry, icon_edge));
                }
                content = content.push(row);
            }
            content = content.push(vertical_spacer(range.after_height));
        }
    }

    let scrollbar_region = ScrollbarRegion::PaneIcons(pane.id);
    let scrollbar_visibility = browser.scrollbar_visibility_for(&scrollbar_region);
    let grid_scroll = scrollable(smooth_scroll_content(content, scrollbar_region.clone()))
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
            Message::IconGridScrolled(pane.id, offset.y, bounds.width, bounds.height)
        });

    mouse_area(
        container(grid_scroll)
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

fn icon_grid_entry<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
    entry: &DirectoryEntry,
    icon_edge: u32,
) -> Element<'a, Message> {
    let visual_state = FileEntryVisualState::from_entry_context(pane, &entry.path, false);
    let icon_tone = visual_state.icon_tone();
    let icon = container(entry_thumbnail_or_icon(
        browser,
        entry,
        icon_tone,
        FileEntryIconDensity::Grid(icon_edge),
    ))
    .width(Length::Fixed(icon_edge as f32))
    .height(Length::Fixed(icon_edge as f32))
    .center_x(Length::Fixed(icon_edge as f32))
    .center_y(Length::Fixed(icon_edge as f32));

    let label: Element<'a, Message> = if pane.renaming == Some(&entry.path) {
        container(
            text_input(
                &crate::localization::translate_current("File name"),
                pane.rename_input,
            )
            .id(rename_input_id())
            .on_input(Message::RenameInputChanged)
            .on_submit(Message::RenameSelected)
            .padding(4)
            .size(ICON_GRID_LABEL_SIZE)
            .width(Length::Fill),
        )
        .height(Length::Fixed(ICON_GRID_LABEL_HEIGHT))
        .clip(true)
        .into()
    } else {
        container(
            readable_text(entry.name().to_string_lossy())
                .size(ICON_GRID_LABEL_SIZE)
                .line_height(text::LineHeight::Relative(1.2))
                .wrapping(text::Wrapping::WordOrGlyph)
                .align_x(alignment::Horizontal::Center)
                .align_y(alignment::Vertical::Top)
                .width(Length::Fill)
                .height(Length::Fixed(ICON_GRID_LABEL_HEIGHT)),
        )
        .width(Length::Fill)
        .height(Length::Fixed(ICON_GRID_LABEL_HEIGHT))
        .clip(true)
        .into()
    };

    let tile_content = Column::new()
        .spacing(ICON_GRID_ICON_LABEL_SPACING)
        .align_x(Alignment::Center)
        .push(icon)
        .push(label);
    let tile = container(tile_content)
        .padding(ICON_GRID_TILE_PADDING)
        .width(Length::Fixed(tile_width(icon_edge)))
        .height(Length::Fixed(tile_visual_height(icon_edge)));
    let tile = match visual_state.row_style_for_selection_run(None) {
        Some(style) => tile.style(style),
        None => tile,
    };

    let tile_area = mouse_area(tile)
        .on_enter(Message::EntryHovered(pane.id, entry.path.clone()))
        .on_exit(Message::EntryHoverCleared(pane.id, entry.path.clone()))
        .on_press(Message::FlatEntryClicked(pane.id, entry.path.clone()))
        .on_release(Message::EntryReleased(pane.id, entry.path.clone()))
        .on_right_press(Message::EntryRightClicked(pane.id, entry.path.clone()))
        .interaction(iced::mouse::Interaction::Pointer);
    let tile_area = if entry.kind == FileKind::Directory && !pane.is_trash_view {
        tile_area.on_middle_press(Message::OpenDirectoryFromMiddleClick(
            pane.id,
            entry.path.clone(),
        ))
    } else {
        tile_area
    };

    track_column_entry_bounds(tile_area, pane.id, entry.path.clone())
}

fn grid_message(message: &'static str) -> Element<'static, Message> {
    container(readable_text(message).size(ICON_GRID_LABEL_SIZE))
        .width(Length::Fill)
        .into()
}

fn vertical_spacer(height: f32) -> Element<'static, Message> {
    Space::new().height(Length::Fixed(height.max(0.0))).into()
}
