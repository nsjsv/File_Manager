use std::path::{Path, PathBuf};

use file_core::{DirectoryEntry, FileKind};
use iced::widget::{container, mouse_area, row, scrollable, text_input, Column, Row, Space};
use iced::{Alignment, Element, Length};

use crate::app::panes::{BrowserPaneView, DirectoryContentAvailability};
use crate::app::smooth_scroll::{smooth_scroll_content_with_shift, smooth_scroll_id};
use crate::app::FileBrowser;
use crate::appearance::{
    auto_hide_horizontal_scrollbar_direction, auto_hide_scrollbar_style,
    auto_hide_vertical_scrollbar_direction, column_browser_style, column_panel_style,
    column_resize_divider_style,
};
use crate::column_entry_bounds::track_column_entry_bounds;
use crate::file_drag_hit_test_bounds::FileDragHitTestMarker;
use crate::file_drag_hit_test_marker::track_file_drag_hit_test_marker;
use crate::file_entry_presentation::SelectionRunPosition;
use crate::file_entry_view::{
    entry_text_input_style, entry_thumbnail_or_icon, themed_icon, FileEntryIconDensity,
    FileEntryVisualState,
};
use crate::icons::IconSymbol;
use crate::input_blocking_space::input_blocking_space;
use crate::measured_middle_ellipsized_text::measured_middle_ellipsized_text;
use crate::model::{
    BrowserPaneId, BrowserPaneLayout, ExpandedDirectoryStatus, Message, ScrollbarRegion, SplitAxis,
};
use crate::typography::readable_text;
use crate::view::{column_browser_scroll_id, rename_input_id, translated_with_width_overflow};
use crate::virtual_range::{initial_virtual_range, virtual_range_for_viewport, VirtualRange};

pub(crate) const DEFAULT_VISIBLE_COLUMN_COUNT: usize = 4;
pub(crate) const COLUMN_RESIZE_DIVIDER_WIDTH: f32 = 5.0;
const COLUMN_RESIZE_LINE_WIDTH: f32 = 1.0;
const CHEVRON_ICON_SIZE: f32 = 11.0;
const COLUMN_CONTENT_SPACING: u32 = 2;
const COLUMN_PADDING: [u16; 2] = [5, 5];
pub(crate) const COLUMN_ENTRIES_TOP_PADDING: f32 =
    COLUMN_PADDING[0] as f32 + COLUMN_CONTENT_SPACING as f32;
const COLUMN_ENTRY_TEXT_SIZE: u32 = 13;
pub(crate) const COLUMN_ENTRY_HEIGHT: f32 = 24.0;
pub(crate) const COLUMN_ENTRY_SCROLL_HEIGHT: f32 =
    COLUMN_ENTRY_HEIGHT + COLUMN_CONTENT_SPACING as f32;
const COLUMN_OVERSCAN_ROWS: usize = 16;
const COLUMN_INITIAL_ROWS: usize = COLUMN_OVERSCAN_ROWS * 2 + 1;
const COLUMN_ENTRY_SPACING: u32 = 4;
const COLUMN_ENTRY_PADDING: [u16; 2] = [1, 4];

pub(crate) fn column_virtual_range_for_viewport(
    total_rows: usize,
    viewport_offset: f32,
    viewport_height: f32,
    overscan_rows: usize,
) -> VirtualRange {
    let viewport_top = viewport_offset.max(0.0);
    let entry_viewport_top = (viewport_top - COLUMN_ENTRIES_TOP_PADDING).max(0.0);
    let entry_viewport_bottom =
        (viewport_top + viewport_height - COLUMN_ENTRIES_TOP_PADDING).max(0.0);
    virtual_range_for_viewport(
        total_rows,
        COLUMN_ENTRY_SCROLL_HEIGHT,
        entry_viewport_top,
        entry_viewport_bottom - entry_viewport_top,
        overscan_rows,
    )
}

pub(crate) fn column_browser_view<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
) -> Element<'a, Message> {
    let rendered_directories = column_directories_for_pane(pane);
    let visible_column_count = rendered_directories.len().max(DEFAULT_VISIBLE_COLUMN_COUNT);
    let sidebar_underlay_width = sidebar_underlay_width_for_pane(browser, pane.id);
    let mut columns = Row::new().spacing(0).height(Length::Fill);
    if sidebar_underlay_width > f32::EPSILON {
        columns = columns.push(
            Space::new()
                .width(Length::Fixed(sidebar_underlay_width))
                .height(Length::Fill),
        );
    }

    for index in 0..visible_column_count {
        if let Some(directory) = rendered_directories.get(index) {
            let active_child =
                active_child_for_column(pane, directory, rendered_directories.get(index + 1));
            columns = columns.push(directory_column(
                browser,
                pane,
                index,
                directory,
                active_child.as_deref(),
            ));
        } else {
            columns = columns.push(empty_column(
                pane.id,
                pane.current_dir.clone(),
                browser.column_width(index),
            ));
        }

        if index + 1 < visible_column_count {
            columns = columns.push(column_resize_divider(pane.id, index));
        }
    }
    let scroll_spacer_width = end_scroll_spacer_width(rendered_directories.len(), |index| {
        browser.column_width(index)
    });
    if scroll_spacer_width > f32::EPSILON {
        columns = columns.push(end_scroll_spacer(scroll_spacer_width));
    }

    let scrollbar_region = ScrollbarRegion::ColumnBrowser(pane.id);
    let scrollbar_visibility = browser.scrollbar_visibility_for(&scrollbar_region);
    let column_content: Element<'_, Message> = scrollable(smooth_scroll_content_with_shift(
        columns,
        scrollbar_region.clone(),
        browser.smooth_scroll_shift_pressed(),
    ))
    .id(column_browser_scroll_id(pane.id))
    .direction(auto_hide_horizontal_scrollbar_direction(
        scrollbar_visibility,
        8.0,
    ))
    .style(auto_hide_scrollbar_style(scrollbar_visibility))
    .width(Length::Fill)
    .height(Length::Fill)
    .on_scroll(move |viewport| {
        let offset = viewport.absolute_offset();
        let bounds = viewport.bounds();
        Message::ColumnBrowserScrolled(pane.id, offset.x, bounds.width)
    })
    .into();
    let column_content = if sidebar_underlay_width > f32::EPSILON {
        translated_with_width_overflow(
            column_content,
            -sidebar_underlay_width,
            0.0,
            sidebar_underlay_width,
        )
    } else {
        column_content
    };

    mouse_area(
        container(column_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(column_browser_style),
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

pub(crate) fn sidebar_underlay_width_for_pane(
    browser: &FileBrowser,
    pane_id: BrowserPaneId,
) -> f32 {
    if pane_starts_at_sidebar_edge(browser.pane_layout, pane_id) {
        browser.sidebar_width
    } else {
        0.0
    }
}

fn pane_starts_at_sidebar_edge(layout: BrowserPaneLayout, pane_id: BrowserPaneId) -> bool {
    match layout {
        BrowserPaneLayout::Single { active } => active == pane_id,
        BrowserPaneLayout::Split {
            axis: SplitAxis::Horizontal,
            first,
            ..
        } => first == pane_id,
        BrowserPaneLayout::Split {
            axis: SplitAxis::Vertical,
            first,
            second,
            ..
        } => first == pane_id || second == pane_id,
    }
}

fn directory_column<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
    column_index: usize,
    directory: &Path,
    active_child: Option<&Path>,
) -> Element<'a, Message> {
    let mut content = Column::new()
        .spacing(COLUMN_CONTENT_SPACING)
        .padding(COLUMN_PADDING)
        .width(Length::Fill);

    match column_content(pane, directory) {
        ColumnContent::Entries(entries) => {
            let range = pane
                .column_viewports
                .get(directory)
                .map(|viewport| {
                    column_virtual_range_for_viewport(
                        entries.len(),
                        viewport.offset_y,
                        viewport.height,
                        COLUMN_OVERSCAN_ROWS,
                    )
                })
                .unwrap_or_else(|| {
                    initial_virtual_range(
                        entries.len(),
                        COLUMN_ENTRY_SCROLL_HEIGHT,
                        COLUMN_INITIAL_ROWS,
                    )
                });
            content = content.push(vertical_spacer(range.before_height));
            for entry_index in range.start..range.end {
                let Some(entry) = entries.get(entry_index) else {
                    break;
                };
                content = content.push(column_entry_row(
                    browser,
                    pane,
                    entries,
                    entry_index,
                    entry,
                    active_child,
                ));
            }
            content = content.push(vertical_spacer(range.after_height));
        }
        ColumnContent::Pending => {}
        ColumnContent::Empty => {
            let message = if pane.is_trash_view {
                "Trash is empty"
            } else {
                "No items"
            };
            content = content.push(column_message(message));
        }
    }

    let scrollbar_region = ScrollbarRegion::Column {
        pane_id: pane.id,
        directory: directory.to_path_buf(),
    };
    let scrollbar_visibility = browser.scrollbar_visibility_for(&scrollbar_region);
    let scroll_directory = directory.to_path_buf();
    let column_scroll = scrollable(smooth_scroll_content_with_shift(
        content,
        scrollbar_region.clone(),
        browser.smooth_scroll_shift_pressed(),
    ))
    .id(smooth_scroll_id(&scrollbar_region))
    .direction(auto_hide_vertical_scrollbar_direction(
        scrollbar_visibility,
        8.0,
    ))
    .height(Length::Fill)
    .style(auto_hide_scrollbar_style(scrollbar_visibility))
    .on_scroll(move |viewport| {
        let offset = viewport.absolute_offset();
        let bounds = viewport.bounds();
        Message::ColumnScrolled(pane.id, scroll_directory.clone(), offset.y, bounds.height)
    });

    let column = container(column_scroll)
        .width(Length::Fixed(browser.column_width(column_index)))
        .height(Length::Fill)
        .style(column_panel_style);

    let directory = directory.to_path_buf();
    track_file_drag_hit_test_marker(
        mouse_area(column)
            .on_enter(Message::DropTargetHovered(pane.id, directory.clone()))
            .on_exit(Message::DropTargetHoverCleared(pane.id, directory.clone()))
            .on_press(Message::ColumnBlankClicked(pane.id, directory.clone()))
            .on_release(Message::DropTargetReleased(pane.id, directory.clone()))
            .on_right_press(Message::ColumnBlankRightClicked(pane.id, directory.clone())),
        FileDragHitTestMarker::DirectoryTarget {
            pane_id: pane.id,
            directory,
        },
    )
}

fn empty_column(
    pane_id: BrowserPaneId,
    fallback_directory: PathBuf,
    width: f32,
) -> Element<'static, Message> {
    track_file_drag_hit_test_marker(
        mouse_area(
            container(Space::new().height(Length::Fill))
                .width(Length::Fixed(width))
                .height(Length::Fill)
                .style(column_panel_style),
        )
        .on_press(Message::ColumnPlaceholderPressed(pane_id))
        .on_release(Message::DropTargetReleased(
            pane_id,
            fallback_directory.clone(),
        ))
        .on_right_press(Message::BlankAreaRightClicked(
            pane_id,
            fallback_directory.clone(),
        )),
        FileDragHitTestMarker::DirectoryTarget {
            pane_id,
            directory: fallback_directory,
        },
    )
}

fn end_scroll_spacer(width: f32) -> Element<'static, Message> {
    input_blocking_space(Length::Fixed(width), Length::Fill)
}

fn end_scroll_spacer_width(
    real_column_count: usize,
    column_width_at: impl Fn(usize) -> f32,
) -> f32 {
    if real_column_count < DEFAULT_VISIBLE_COLUMN_COUNT {
        return 0.0;
    }

    (real_column_count..real_column_count + DEFAULT_VISIBLE_COLUMN_COUNT - 1)
        .map(|index| column_width_at(index) + COLUMN_RESIZE_DIVIDER_WIDTH)
        .sum()
}

fn column_resize_divider(pane_id: BrowserPaneId, column_index: usize) -> Element<'static, Message> {
    let visible_line = container(Space::new().width(Length::Fixed(COLUMN_RESIZE_LINE_WIDTH)))
        .width(Length::Fixed(COLUMN_RESIZE_LINE_WIDTH))
        .height(Length::Fill)
        .style(column_resize_divider_style);
    let divider = row![
        Space::new().width(Length::Fill),
        visible_line,
        Space::new().width(Length::Fill),
    ]
    .width(Length::Fixed(COLUMN_RESIZE_DIVIDER_WIDTH))
    .height(Length::Fill);

    mouse_area(divider)
        .on_press(Message::ColumnResizeStarted(pane_id, column_index))
        .on_release(Message::DragSelectionFinished)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into()
}

fn column_message(message: &'static str) -> Element<'static, Message> {
    container(readable_text(message).size(COLUMN_ENTRY_TEXT_SIZE))
        .padding(COLUMN_ENTRY_PADDING)
        .width(Length::Fill)
        .into()
}

fn vertical_spacer(height: f32) -> Element<'static, Message> {
    Space::new().height(Length::Fixed(height.max(0.0))).into()
}

fn column_entry_row<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
    entries: &[DirectoryEntry],
    entry_index: usize,
    entry: &DirectoryEntry,
    active_child: Option<&Path>,
) -> Element<'a, Message> {
    let visual_state = FileEntryVisualState::from_entry_context(
        pane,
        &entry.path,
        active_child == Some(entry.path.as_path()),
    );
    let modifier = browser.file_entry_content_modifier(&entry.path);
    let icon_tone = visual_state.icon_tone();

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
        container(measured_middle_ellipsized_text(
            entry.name().to_string_lossy().into_owned(),
            COLUMN_ENTRY_TEXT_SIZE,
        ))
        .style(visual_state.content_style(modifier))
        .into()
    };

    let trailing: Element<'static, Message> =
        if entry.kind == FileKind::Directory && !pane.is_trash_view {
            themed_icon(IconSymbol::ChevronRight, icon_tone, CHEVRON_ICON_SIZE).into()
        } else {
            Space::new().width(Length::Fixed(CHEVRON_ICON_SIZE)).into()
        };

    let row_content = row![
        entry_thumbnail_or_icon(browser, entry, icon_tone, FileEntryIconDensity::Column),
        name,
        trailing
    ]
    .spacing(COLUMN_ENTRY_SPACING)
    .align_y(Alignment::Center);
    let row_container = container(row_content)
        .padding(COLUMN_ENTRY_PADDING)
        .height(Length::Fixed(COLUMN_ENTRY_HEIGHT))
        .center_y(Length::Fixed(COLUMN_ENTRY_HEIGHT))
        .width(Length::Fill);
    let selection_run_position =
        selection_run_position_for_entry_index(entries, entry_index, pane.selected_paths);
    let row_container = match visual_state.row_style_for_selection_run(selection_run_position) {
        Some(style) => row_container.style(style),
        None => row_container,
    };

    let row_area = mouse_area(row_container)
        .on_enter(Message::EntryHovered(pane.id, entry.path.clone()))
        .on_exit(Message::EntryHoverCleared(pane.id, entry.path.clone()))
        .on_press(Message::ColumnEntryClicked(pane.id, entry.path.clone()))
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

    let row_element: Element<'a, Message> = row_area.into();
    track_column_entry_bounds(row_element, pane.id, entry.path.clone())
}

fn selection_run_position_for_entry_index(
    entries: &[DirectoryEntry],
    index: usize,
    selected_paths: &std::collections::HashSet<PathBuf>,
) -> Option<SelectionRunPosition> {
    let entry = entries.get(index)?;
    if !selected_paths.contains(&entry.path) {
        return None;
    }
    let previous_selected = index
        .checked_sub(1)
        .and_then(|previous| entries.get(previous))
        .is_some_and(|previous| selected_paths.contains(&previous.path));
    let next_selected = entries
        .get(index + 1)
        .is_some_and(|next| selected_paths.contains(&next.path));
    Some(SelectionRunPosition::from_neighbors(
        previous_selected,
        next_selected,
    ))
}

fn column_content<'a>(pane: BrowserPaneView<'a>, directory: &Path) -> ColumnContent<'a> {
    if directory == pane.current_dir.as_path() {
        return match pane.current_directory_content() {
            DirectoryContentAvailability::Pending => ColumnContent::Pending,
            DirectoryContentAvailability::Available([]) => ColumnContent::Empty,
            DirectoryContentAvailability::Available(entries) => ColumnContent::Entries(entries),
        };
    }

    match pane.expanded_directories.get(directory) {
        Some(expanded) => match &expanded.status {
            ExpandedDirectoryStatus::Loading if expanded.entries.is_empty() => {
                ColumnContent::Pending
            }
            ExpandedDirectoryStatus::Loading => ColumnContent::Entries(&expanded.entries),
            ExpandedDirectoryStatus::Loaded if expanded.entries.is_empty() => ColumnContent::Empty,
            ExpandedDirectoryStatus::Loaded => ColumnContent::Entries(&expanded.entries),
            ExpandedDirectoryStatus::Error => ColumnContent::Empty,
        },
        None => ColumnContent::Pending,
    }
}

pub(crate) fn column_directories(browser: &FileBrowser) -> Vec<PathBuf> {
    let Some(pane) = browser.pane_view(browser.active_pane_id()) else {
        return Vec::new();
    };
    column_directories_for_pane(pane)
}

pub(crate) fn column_directories_for_pane(pane: BrowserPaneView<'_>) -> Vec<PathBuf> {
    if pane.is_trash_view {
        return vec![pane.current_dir.clone()];
    }

    if let Some(directories) = pane
        .file_drag
        .and_then(|drag| drag.source_column_directories(pane.id, pane.active_tab_id))
    {
        return directories.to_vec();
    }
    let mut directories = vec![pane.current_dir.clone()];
    if let Some(open_directory) = pane.deepest_open_column_directory {
        append_column_directory_chain(
            &mut directories,
            pane.current_dir.as_path(),
            open_directory.as_path(),
        );
    }
    directories
}

fn append_column_directory_chain(
    directories: &mut Vec<PathBuf>,
    current_dir: &Path,
    selected_directory: &Path,
) {
    if selected_directory == current_dir || !selected_directory.starts_with(current_dir) {
        return;
    }

    let mut ancestors = Vec::new();
    let mut cursor = Some(selected_directory);
    while let Some(path) = cursor {
        if path == current_dir {
            break;
        }
        if !path.starts_with(current_dir) {
            break;
        }
        ancestors.push(path.to_path_buf());
        cursor = path.parent();
    }
    ancestors.reverse();
    directories.extend(ancestors);
}

fn active_child_for_column(
    pane: BrowserPaneView<'_>,
    directory: &Path,
    next_directory: Option<&PathBuf>,
) -> Option<PathBuf> {
    if let Some(next_directory) = next_directory {
        return Some(next_directory.clone());
    }

    let selected = pane.selected?;
    if selected.parent() == Some(directory) {
        return Some(selected.to_path_buf());
    }
    None
}

enum ColumnContent<'a> {
    Pending,
    Empty,
    Entries(&'a [DirectoryEntry]),
}

#[cfg(test)]
#[path = "three_column_view/directory_content_tests.rs"]
mod directory_content_tests;

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use file_core::{DirectoryEntry, EntryMetadata, FileKind};

    use super::*;

    fn test_entry(path: PathBuf, kind: FileKind) -> DirectoryEntry {
        DirectoryEntry::new(
            path,
            kind,
            EntryMetadata {
                len: 0,
                modified: None,
                ..EntryMetadata::default()
            },
            false,
            false,
            false,
        )
    }

    fn loaded_expanded_directory(entries: Vec<DirectoryEntry>) -> crate::model::ExpandedDirectory {
        crate::model::ExpandedDirectory {
            entries,
            directory_discovery: None,
            status: ExpandedDirectoryStatus::Loaded,
            is_expanded: true,
            is_collapsing: false,
            animation_progress: 1.0,
            load_generation: 0,
            load_context: None,
            load_cancel: None,
            directory_order_phase: crate::model::DirectoryOrderPhase::Ready {
                field: file_core::SortField::Name,
                direction: file_core::SortDirection::Ascending,
            },
        }
    }

    fn test_pane_view<'a>(
        current_dir: &'a PathBuf,
        entries: &'a [DirectoryEntry],
        selected: Option<&'a PathBuf>,
        selected_paths: &'a HashSet<PathBuf>,
        deepest_open_column_directory: Option<&'a PathBuf>,
        expanded_directories: &'a HashMap<PathBuf, crate::model::ExpandedDirectory>,
        column_viewports: &'a HashMap<PathBuf, crate::thumbnail_cache::ColumnViewport>,
    ) -> BrowserPaneView<'a> {
        BrowserPaneView {
            id: BrowserPaneId::PRIMARY,
            current_dir,
            is_trash_view: false,
            entries,
            directory_discovery: None,
            directory_loading_placeholder_entries: &[],
            selected,
            selected_paths,
            deepest_open_column_directory,
            hovered_entry: None,
            expanded_directories,
            column_viewports,
            icon_grid_viewport: crate::model::IconGridViewport::default(),
            view_mode: crate::model::BrowserViewMode::Columns,
            tabs: &[],
            active_tab_id: 0,
            tab_animations: None,
            address_editing: None,
            address_transition_fraction: 0.0,
            address_exit_snapshot: None,
            directory_collection_phase: crate::model::DirectoryCollectionPhase::Ready,
            renaming: None,
            rename_input: "",
            file_drag: None,
            tab_bar_reveal_fraction: 0.0,
        }
    }

    #[test]
    fn column_virtual_range_uses_padding_and_top_spacer_gap() {
        assert_eq!(COLUMN_ENTRIES_TOP_PADDING, 7.0);

        let first = column_virtual_range_for_viewport(
            10,
            COLUMN_ENTRIES_TOP_PADDING,
            COLUMN_ENTRY_HEIGHT,
            0,
        );
        assert_eq!((first.start, first.end), (0, 1));

        let second = column_virtual_range_for_viewport(
            10,
            COLUMN_ENTRIES_TOP_PADDING + COLUMN_ENTRY_SCROLL_HEIGHT,
            COLUMN_ENTRY_HEIGHT,
            0,
        );
        assert_eq!((second.start, second.end), (1, 2));
    }

    #[test]
    fn sidebar_underlay_only_applies_to_panes_touching_sidebar_edge() {
        let first = BrowserPaneId(1);
        let second = BrowserPaneId(2);

        assert!(pane_starts_at_sidebar_edge(
            BrowserPaneLayout::Single { active: first },
            first,
        ));
        assert!(pane_starts_at_sidebar_edge(
            BrowserPaneLayout::Split {
                axis: SplitAxis::Horizontal,
                first,
                second,
                active: first,
            },
            first,
        ));
        assert!(!pane_starts_at_sidebar_edge(
            BrowserPaneLayout::Split {
                axis: SplitAxis::Horizontal,
                first,
                second,
                active: second,
            },
            second,
        ));
        assert!(pane_starts_at_sidebar_edge(
            BrowserPaneLayout::Split {
                axis: SplitAxis::Vertical,
                first,
                second,
                active: second,
            },
            second,
        ));
    }

    #[test]
    fn open_column_context_opens_child_column() {
        let current_dir = PathBuf::from("/workspace");
        let directory = current_dir.join("alpha");
        let entries = vec![test_entry(directory.clone(), FileKind::Directory)];
        let selected_paths = HashSet::from([directory.clone()]);
        let expanded_directories = HashMap::new();
        let column_viewports = HashMap::new();
        let pane = test_pane_view(
            &current_dir,
            &entries,
            Some(&directory),
            &selected_paths,
            Some(&directory),
            &expanded_directories,
            &column_viewports,
        );

        assert_eq!(
            column_directories_for_pane(pane),
            vec![current_dir, directory]
        );
    }

    #[test]
    fn selected_directory_without_open_column_context_does_not_open_child_column() {
        let current_dir = PathBuf::from("/workspace");
        let directory = current_dir.join("alpha");
        let entries = vec![test_entry(directory.clone(), FileKind::Directory)];
        let selected_paths = HashSet::from([directory.clone()]);
        let expanded_directories = HashMap::new();
        let column_viewports = HashMap::new();
        let pane = test_pane_view(
            &current_dir,
            &entries,
            Some(&directory),
            &selected_paths,
            None,
            &expanded_directories,
            &column_viewports,
        );

        assert_eq!(column_directories_for_pane(pane), vec![current_dir]);
    }

    #[test]
    fn multiple_selected_directories_do_not_open_child_column() {
        let current_dir = PathBuf::from("/workspace");
        let first = current_dir.join("alpha");
        let second = current_dir.join("beta");
        let entries = vec![
            test_entry(first.clone(), FileKind::Directory),
            test_entry(second.clone(), FileKind::Directory),
        ];
        let selected_paths = HashSet::from([first, second.clone()]);
        let expanded_directories = HashMap::new();
        let column_viewports = HashMap::new();
        let pane = test_pane_view(
            &current_dir,
            &entries,
            Some(&second),
            &selected_paths,
            None,
            &expanded_directories,
            &column_viewports,
        );

        assert_eq!(column_directories_for_pane(pane), vec![current_dir]);
    }

    #[test]
    fn multi_selection_preserves_open_child_column() {
        let current_dir = PathBuf::from("/workspace");
        let open_directory = current_dir.join("project");
        let selected_file = current_dir.join("notes.txt");
        let entries = vec![
            test_entry(open_directory.clone(), FileKind::Directory),
            test_entry(selected_file.clone(), FileKind::File),
        ];
        let selected_paths = HashSet::from([open_directory.clone(), selected_file.clone()]);
        let expanded_directories = HashMap::from([(
            open_directory.clone(),
            loaded_expanded_directory(Vec::new()),
        )]);
        let column_viewports = HashMap::new();
        let pane = test_pane_view(
            &current_dir,
            &entries,
            Some(&selected_file),
            &selected_paths,
            Some(&open_directory),
            &expanded_directories,
            &column_viewports,
        );

        assert_eq!(
            column_directories_for_pane(pane),
            vec![current_dir, open_directory]
        );
    }

    #[test]
    fn end_scroll_spacer_width_keeps_default_placeholders_and_terminal_scroll() {
        let column_width = 180.0;
        let divider_count = (DEFAULT_VISIBLE_COLUMN_COUNT - 1) as f32;
        let expected_spacer = divider_count * (column_width + COLUMN_RESIZE_DIVIDER_WIDTH);

        for real_column_count in [1, DEFAULT_VISIBLE_COLUMN_COUNT - 1] {
            assert_eq!(
                end_scroll_spacer_width(real_column_count, |_| column_width),
                0.0
            );
        }
        for real_column_count in [DEFAULT_VISIBLE_COLUMN_COUNT, 5] {
            assert_eq!(
                end_scroll_spacer_width(real_column_count, |_| column_width),
                expected_spacer
            );
        }

        let viewport_width = DEFAULT_VISIBLE_COLUMN_COUNT as f32 * column_width
            + divider_count * COLUMN_RESIZE_DIVIDER_WIDTH;
        assert_eq!(
            viewport_width + expected_spacer - viewport_width,
            expected_spacer
        );
    }
}
