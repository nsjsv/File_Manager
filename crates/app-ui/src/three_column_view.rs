#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use file_core::{DirectoryEntry, FileKind};
use iced::widget::{
    container, image, mouse_area, row, scrollable, text_input, Column, Row, Space, Svg,
};
use iced::{Alignment, Element, Length, Padding, Theme};

use crate::app::panes::BrowserPaneView;
use crate::app::FileBrowser;
use crate::appearance::{
    auto_hide_horizontal_scrollbar_direction, auto_hide_scrollbar_style,
    auto_hide_vertical_scrollbar_direction, column_browser_style, column_panel_style,
    column_resize_divider_style, dragged_row_style, hovered_row_style, icon_svg_style,
    muted_icon_svg_style, selected_icon_svg_style, selected_row_style, warning_icon_svg_style,
};
use crate::column_entry_bounds::track_column_entry_bounds;
use crate::icons::{file_entry_icon_symbol, IconSymbol};
use crate::measured_middle_ellipsized_text::measured_middle_ellipsized_text;
use crate::model::{BrowserPaneId, ExpandedDirectoryStatus, Message, TRASH_LOCATION_LABEL};
use crate::thumbnail_cache::{LIST_THUMBNAIL_EDGE, LIST_THUMBNAIL_SIZE};
use crate::typography::readable_text;
use crate::view::{column_browser_scroll_id, rename_input_id};

pub(crate) const DEFAULT_VISIBLE_COLUMN_COUNT: usize = 3;
pub(crate) const COLUMN_RESIZE_DIVIDER_WIDTH: f32 = 6.0;
const COLUMN_RESIZE_LINE_WIDTH: f32 = 1.0;
const ROW_ICON_SIZE: f32 = 18.0;
const CHEVRON_ICON_SIZE: f32 = 15.0;
const COLUMN_CONTENT_SPACING: u32 = 4;
const COLUMN_PADDING: [u16; 2] = [10, 8];
const COLUMN_TITLE_TEXT_SIZE: u32 = 13;
const COLUMN_ENTRY_TEXT_SIZE: u32 = 16;
const COLUMN_ENTRY_SPACING: u32 = 8;
const COLUMN_ENTRY_PADDING: [u16; 2] = [6, 8];

pub(crate) fn column_browser_view<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
) -> Element<'a, Message> {
    let rendered_directories = column_directories_for_pane(pane);
    let visible_column_count = rendered_directories.len().max(DEFAULT_VISIBLE_COLUMN_COUNT);
    let mut columns = Row::new().spacing(0).height(Length::Fill);

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
            columns = columns.push(empty_column(browser.column_width(index)));
        }

        if index + 1 < visible_column_count {
            columns = columns.push(column_resize_divider(pane.id, index));
        }
    }

    let column_content: Element<'_, Message> = scrollable(columns)
        .id(column_browser_scroll_id(pane.id))
        .direction(auto_hide_horizontal_scrollbar_direction(
            browser.scrollbar_visibility,
            8.0,
        ))
        .style(auto_hide_scrollbar_style(browser.scrollbar_visibility))
        .width(Length::Fill)
        .height(Length::Fill)
        .into();

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

    content = content.push(column_title(pane, directory));
    match column_content(pane, directory) {
        ColumnContent::Entries(entries) => {
            for entry in entries {
                content = content.push(column_entry_row(browser, pane, entry, active_child));
            }
        }
        ColumnContent::Loading => {
            content = content.push(column_message("Loading..."));
        }
        ColumnContent::Empty => {
            let message = if pane.is_trash_view {
                "Trash is empty"
            } else {
                "No items"
            };
            content = content.push(column_message(message));
        }
    }

    let scroll_directory = directory.to_path_buf();
    let column_scroll = scrollable(content)
        .id(column_scroll_id(pane.id, directory))
        .direction(auto_hide_vertical_scrollbar_direction(
            browser.scrollbar_visibility,
            8.0,
        ))
        .height(Length::Fill)
        .style(auto_hide_scrollbar_style(browser.scrollbar_visibility))
        .on_scroll(move |viewport| {
            let offset = viewport.absolute_offset();
            let bounds = viewport.bounds();
            Message::ColumnScrolled(pane.id, scroll_directory.clone(), offset.y, bounds.height)
        });

    let column = container(column_scroll)
        .width(Length::Fixed(browser.column_width(column_index)))
        .height(Length::Fill)
        .style(column_panel_style);

    mouse_area(column)
        .on_enter(Message::DropTargetHovered(pane.id, directory.to_path_buf()))
        .on_exit(Message::DropTargetHoverCleared(
            pane.id,
            directory.to_path_buf(),
        ))
        .on_press(Message::ColumnBlankClicked(
            pane.id,
            directory.to_path_buf(),
        ))
        .on_release(Message::DropTargetReleased(
            pane.id,
            directory.to_path_buf(),
        ))
        .on_right_press(Message::BlankAreaRightClicked(
            pane.id,
            directory.to_path_buf(),
        ))
        .into()
}

fn empty_column(width: f32) -> Element<'static, Message> {
    container(Space::new().height(Length::Fill))
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .style(column_panel_style)
        .into()
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

fn column_title(pane: BrowserPaneView<'_>, directory: &Path) -> Element<'static, Message> {
    let title = if pane.is_trash_view && directory == pane.current_dir.as_path() {
        TRASH_LOCATION_LABEL.to_owned()
    } else {
        directory
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| directory.to_string_lossy().into_owned())
    };
    container(measured_middle_ellipsized_text(
        title,
        COLUMN_TITLE_TEXT_SIZE,
    ))
    .padding(column_title_padding())
    .width(Length::Fill)
    .into()
}

fn column_message(message: &'static str) -> Element<'static, Message> {
    container(readable_text(message).size(COLUMN_ENTRY_TEXT_SIZE))
        .padding(COLUMN_ENTRY_PADDING)
        .width(Length::Fill)
        .into()
}

fn column_entry_row<'a>(
    browser: &'a FileBrowser,
    pane: BrowserPaneView<'a>,
    entry: &DirectoryEntry,
    active_child: Option<&Path>,
) -> Element<'a, Message> {
    let is_selected =
        pane.is_path_selected(&entry.path) || active_child == Some(entry.path.as_path());
    let is_hovered = pane.hovered_entry == Some(&entry.path);
    let is_dragged = is_drag_source(pane, &entry.path);
    let icon_tone = if is_dragged {
        IconTone::Muted
    } else if is_selected {
        IconTone::Selected
    } else {
        IconTone::Normal
    };

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
            COLUMN_ENTRY_TEXT_SIZE,
        )
    };

    let trailing: Element<'static, Message> =
        if entry.kind == FileKind::Directory && !pane.is_trash_view {
            themed_icon(IconSymbol::ChevronRight, icon_tone, CHEVRON_ICON_SIZE).into()
        } else {
            Space::new().width(Length::Fixed(CHEVRON_ICON_SIZE)).into()
        };

    let row_content = row![
        entry_thumbnail_or_icon(browser, entry, icon_tone),
        name,
        trailing
    ]
    .spacing(COLUMN_ENTRY_SPACING)
    .align_y(Alignment::Center);
    let row_container = container(row_content)
        .padding(COLUMN_ENTRY_PADDING)
        .width(Length::Fill);
    let row_container = if is_dragged {
        row_container.style(dragged_row_style)
    } else if is_selected {
        row_container.style(selected_row_style)
    } else if is_hovered {
        row_container.style(hovered_row_style)
    } else {
        row_container
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

fn is_drag_source(pane: BrowserPaneView<'_>, path: &Path) -> bool {
    pane.file_drag.is_some_and(|drag| {
        drag.is_dragging() && drag.sources.iter().any(|source| source.as_path() == path)
    })
}

fn column_content<'a>(pane: BrowserPaneView<'a>, directory: &Path) -> ColumnContent<'a> {
    if directory == pane.current_dir.as_path() {
        if pane.is_loading && pane.entries.is_empty() {
            return ColumnContent::Loading;
        }
        if pane.entries.is_empty() {
            return ColumnContent::Empty;
        }
        return ColumnContent::Entries(pane.entries);
    }

    match pane.expanded_directories.get(directory) {
        Some(expanded) => match &expanded.status {
            ExpandedDirectoryStatus::Loading if expanded.entries.is_empty() => {
                ColumnContent::Loading
            }
            ExpandedDirectoryStatus::Loading => ColumnContent::Entries(&expanded.entries),
            ExpandedDirectoryStatus::Loaded if expanded.entries.is_empty() => ColumnContent::Empty,
            ExpandedDirectoryStatus::Loaded => ColumnContent::Entries(&expanded.entries),
            ExpandedDirectoryStatus::Error => ColumnContent::Empty,
        },
        None => ColumnContent::Loading,
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

    if let Some(drag) = pane.file_drag {
        if !drag.column_directories_snapshot.is_empty() {
            return drag.column_directories_snapshot.clone();
        }
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

fn column_title_padding() -> Padding {
    Padding {
        top: 0.0,
        right: 7.0,
        bottom: 6.0,
        left: 7.0,
    }
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

fn entry_thumbnail_or_icon<'a>(
    browser: &'a FileBrowser,
    entry: &DirectoryEntry,
    tone: IconTone,
) -> Element<'a, Message> {
    if let Some(thumbnail) = browser
        .thumbnail_cache
        .ready_for_entry(entry, LIST_THUMBNAIL_EDGE)
    {
        return container(
            image::Image::new(thumbnail.handle.clone())
                .width(Length::Fixed(LIST_THUMBNAIL_SIZE))
                .height(Length::Fixed(LIST_THUMBNAIL_SIZE)),
        )
        .width(Length::Fixed(LIST_THUMBNAIL_SIZE))
        .height(Length::Fixed(LIST_THUMBNAIL_SIZE))
        .into();
    }

    if !thumbnails::is_supported_thumbnail_path(&entry.path) {
        return entry_icon(entry, tone).into();
    }

    container(entry_icon(entry, tone))
        .width(Length::Fixed(LIST_THUMBNAIL_SIZE))
        .height(Length::Fixed(LIST_THUMBNAIL_SIZE))
        .center_x(Length::Fixed(LIST_THUMBNAIL_SIZE))
        .center_y(Length::Fixed(LIST_THUMBNAIL_SIZE))
        .into()
}

fn entry_icon(entry: &DirectoryEntry, tone: IconTone) -> Svg<'static, Theme> {
    let symbol = if entry.kind == FileKind::Symlink && entry.is_broken_symlink {
        IconSymbol::TriangleAlert
    } else {
        file_entry_icon_symbol(entry.kind, entry.name())
    };
    let tone = match (symbol, tone) {
        (IconSymbol::TriangleAlert, IconTone::Muted) => IconTone::Muted,
        (IconSymbol::TriangleAlert, _) => IconTone::Warning,
        _ => tone,
    };
    themed_icon(symbol, tone, ROW_ICON_SIZE)
}

fn column_scroll_id(pane_id: BrowserPaneId, directory: &Path) -> iced::widget::Id {
    iced::widget::Id::from(format!(
        "column-scroll-{}-{}",
        pane_id.key(),
        path_hash(directory)
    ))
}

fn path_hash(path: &Path) -> String {
    #[cfg(unix)]
    {
        hash_bytes(path.as_os_str().as_bytes())
    }
    #[cfg(not(unix))]
    {
        hash_bytes(path.to_string_lossy().as_bytes())
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn themed_icon(symbol: IconSymbol, tone: IconTone, size: f32) -> Svg<'static, Theme> {
    symbol.view(size).style(icon_tone_style(tone))
}

fn icon_tone_style(
    tone: IconTone,
) -> fn(&Theme, iced::widget::svg::Status) -> iced::widget::svg::Style {
    match tone {
        IconTone::Normal => icon_svg_style(),
        IconTone::Selected => selected_icon_svg_style(),
        IconTone::Muted => muted_icon_svg_style(),
        IconTone::Warning => warning_icon_svg_style(),
    }
}

enum ColumnContent<'a> {
    Loading,
    Empty,
    Entries(&'a [DirectoryEntry]),
}

#[derive(Debug, Clone, Copy)]
enum IconTone {
    Normal,
    Selected,
    Muted,
    Warning,
}

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
                readonly: false,
            },
            false,
            false,
            false,
        )
    }

    fn loaded_expanded_directory(entries: Vec<DirectoryEntry>) -> crate::model::ExpandedDirectory {
        crate::model::ExpandedDirectory {
            entries,
            status: ExpandedDirectoryStatus::Loaded,
            is_expanded: true,
            animation_progress: 1.0,
        }
    }

    fn test_pane_view<'a>(
        current_dir: &'a PathBuf,
        entries: &'a [DirectoryEntry],
        selected: Option<&'a PathBuf>,
        selected_paths: &'a HashSet<PathBuf>,
        deepest_open_column_directory: Option<&'a PathBuf>,
        expanded_directories: &'a HashMap<PathBuf, crate::model::ExpandedDirectory>,
    ) -> BrowserPaneView<'a> {
        BrowserPaneView {
            id: BrowserPaneId::PRIMARY,
            current_dir,
            is_trash_view: false,
            entries,
            selected,
            selected_paths,
            deepest_open_column_directory,
            hovered_entry: None,
            expanded_directories,
            tabs: &[],
            active_tab_id: 0,
            tab_animations: None,
            path_input: "",
            path_suggestions: &[],
            path_suggestion_selection: None,
            is_loading: false,
            renaming: None,
            rename_input: "",
            file_drag: None,
            tab_bar_reveal_fraction: 0.0,
        }
    }

    #[test]
    fn open_column_context_opens_child_column() {
        let current_dir = PathBuf::from("/workspace");
        let directory = current_dir.join("alpha");
        let entries = vec![test_entry(directory.clone(), FileKind::Directory)];
        let selected_paths = HashSet::from([directory.clone()]);
        let expanded_directories = HashMap::new();
        let pane = test_pane_view(
            &current_dir,
            &entries,
            Some(&directory),
            &selected_paths,
            Some(&directory),
            &expanded_directories,
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
        let pane = test_pane_view(
            &current_dir,
            &entries,
            Some(&directory),
            &selected_paths,
            None,
            &expanded_directories,
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
        let pane = test_pane_view(
            &current_dir,
            &entries,
            Some(&second),
            &selected_paths,
            None,
            &expanded_directories,
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
        let pane = test_pane_view(
            &current_dir,
            &entries,
            Some(&selected_file),
            &selected_paths,
            Some(&open_directory),
            &expanded_directories,
        );

        assert_eq!(
            column_directories_for_pane(pane),
            vec![current_dir, open_directory]
        );
    }
}
