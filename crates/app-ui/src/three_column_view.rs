#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use file_core::{DirectoryEntry, FileKind};
use iced::widget::{
    container, image, mouse_area, row, scrollable, text_input, Column, Row, Space, Svg,
};
use iced::{Alignment, Element, Length, Theme};

use crate::app::FileBrowser;
use crate::appearance::{
    auto_hide_horizontal_scrollbar_direction, auto_hide_scrollbar_style,
    auto_hide_vertical_scrollbar_direction, column_browser_style, column_panel_style,
    column_resize_divider_style, dragged_row_style, hovered_row_style, icon_svg_style,
    muted_icon_svg_style, selected_icon_svg_style, selected_row_style, warning_icon_svg_style,
};
use crate::icons::{file_entry_icon_symbol, IconSymbol};
use crate::measured_middle_ellipsized_text::measured_middle_ellipsized_text;
use crate::model::{ColumnViewMode, ExpandedDirectoryStatus, Message, TRASH_LOCATION_LABEL};
use crate::sidebar::SIDEBAR_WIDTH;
use crate::thumbnail_cache::{LIST_THUMBNAIL_EDGE, LIST_THUMBNAIL_SIZE};
use crate::typography::readable_text;
use crate::view::{column_browser_scroll_id, rename_input_id};

const COLUMN_RESIZE_DIVIDER_WIDTH: f32 = 6.0;
const ROW_ICON_SIZE: f32 = 18.0;
const CHEVRON_ICON_SIZE: f32 = 15.0;
const COMPRESSED_UNBOUNDED_COLUMN_WIDTH: f32 = 124.0;
const MIN_FIXED_COLUMN_WIDTH: f32 = 180.0;

pub(crate) fn column_browser_view(browser: &FileBrowser) -> Element<'_, Message> {
    let rendered_directories = column_directories(browser);
    let fixed_count = browser.column_fixed_count.max(1);
    let focused_index = rendered_directories.len().saturating_sub(1);
    let mut columns = Row::new().spacing(0).height(Length::Fill);

    for (index, directory) in rendered_directories.iter().enumerate() {
        let active_child =
            active_child_for_column(browser, directory, rendered_directories.get(index + 1));
        let presentation = column_presentation(browser.column_view_mode, index, focused_index);
        columns = columns.push(directory_column(
            browser,
            directory,
            active_child.as_deref(),
            presentation,
        ));

        if browser.column_view_mode == ColumnViewMode::Unbounded
            && index + 1 < rendered_directories.len()
        {
            columns = columns.push(column_resize_divider());
        }
    }

    if browser.column_view_mode == ColumnViewMode::Fixed {
        let placeholder_width = fixed_column_width(browser);
        for _ in rendered_directories.len()..fixed_count {
            columns = columns.push(empty_fixed_column(placeholder_width));
        }
    }

    let column_content: Element<'_, Message> = scrollable(columns)
        .id(column_browser_scroll_id())
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
    .on_press(Message::BlankAreaPressed)
    .on_release(Message::DragSelectionFinished)
    .on_right_press(Message::BlankAreaRightClicked(browser.current_dir.clone()))
    .on_enter(Message::ColumnBrowserCursorEntered)
    .on_exit(Message::ColumnBrowserCursorExited)
    .into()
}

fn directory_column<'a>(
    browser: &'a FileBrowser,
    directory: &Path,
    active_child: Option<&Path>,
    presentation: ColumnPresentation,
) -> Element<'a, Message> {
    let mut content = Column::new()
        .spacing(presentation.content_spacing())
        .padding(presentation.column_padding())
        .width(Length::Fill);

    content = content.push(column_title(browser, directory, presentation));
    match column_content(browser, directory) {
        ColumnContent::Entries(entries) => {
            for entry in entries {
                content =
                    content.push(column_entry_row(browser, entry, active_child, presentation));
            }
        }
        ColumnContent::Loading => {
            content = content.push(column_message("Loading...", presentation));
        }
        ColumnContent::Empty => {
            let message = if browser.is_trash_view {
                "Trash is empty"
            } else {
                "No items"
            };
            content = content.push(column_message(message, presentation));
        }
    }

    let scroll_directory = directory.to_path_buf();
    let column_scroll = scrollable(content)
        .id(column_scroll_id(directory))
        .direction(auto_hide_vertical_scrollbar_direction(
            browser.scrollbar_visibility,
            8.0,
        ))
        .height(Length::Fill)
        .style(auto_hide_scrollbar_style(browser.scrollbar_visibility))
        .on_scroll(move |viewport| {
            let offset = viewport.absolute_offset();
            let bounds = viewport.bounds();
            Message::ColumnScrolled(scroll_directory.clone(), offset.y, bounds.height)
        });

    let column = container(column_scroll)
        .width(presentation.width(browser))
        .height(Length::Fill)
        .style(column_panel_style);

    mouse_area(column)
        .on_enter(Message::DropTargetHovered(directory.to_path_buf()))
        .on_exit(Message::DropTargetHoverCleared(directory.to_path_buf()))
        .on_press(Message::ColumnBlankClicked(directory.to_path_buf()))
        .on_release(Message::DragSelectionFinished)
        .on_right_press(Message::BlankAreaRightClicked(directory.to_path_buf()))
        .into()
}

fn empty_fixed_column(width: f32) -> Element<'static, Message> {
    container(Space::with_height(Length::Fill))
        .width(Length::Fixed(width))
        .height(Length::Fill)
        .style(column_panel_style)
        .into()
}

fn column_resize_divider() -> Element<'static, Message> {
    let divider = container(Space::with_width(Length::Fixed(
        COLUMN_RESIZE_DIVIDER_WIDTH,
    )))
    .width(Length::Fixed(COLUMN_RESIZE_DIVIDER_WIDTH))
    .height(Length::Fill)
    .style(column_resize_divider_style);

    mouse_area(divider)
        .on_press(Message::ColumnResizeStarted)
        .on_release(Message::DragSelectionFinished)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into()
}

fn column_title(
    browser: &FileBrowser,
    directory: &Path,
    presentation: ColumnPresentation,
) -> Element<'static, Message> {
    let title = if browser.is_trash_view && directory == browser.current_dir {
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
        presentation.title_text_size(),
    ))
    .padding(presentation.title_padding())
    .width(Length::Fill)
    .into()
}

fn column_message(
    message: &'static str,
    presentation: ColumnPresentation,
) -> Element<'static, Message> {
    container(readable_text(message).size(presentation.entry_text_size()))
        .padding(presentation.entry_padding())
        .width(Length::Fill)
        .into()
}

fn column_entry_row<'a>(
    browser: &'a FileBrowser,
    entry: &DirectoryEntry,
    active_child: Option<&Path>,
    presentation: ColumnPresentation,
) -> Element<'a, Message> {
    let is_selected =
        browser.is_path_selected(&entry.path) || active_child == Some(entry.path.as_path());
    let is_hovered = browser.hovered_entry.as_ref() == Some(&entry.path);
    let is_dragged = is_drag_source(browser, &entry.path);
    let icon_tone = if is_dragged {
        IconTone::Muted
    } else if is_selected {
        IconTone::Selected
    } else {
        IconTone::Normal
    };

    let name: Element<'a, Message> = if browser.renaming.as_ref() == Some(&entry.path) {
        text_input("File name", &browser.rename_input)
            .id(rename_input_id())
            .on_input(Message::RenameInputChanged)
            .on_submit(Message::RenameSelected)
            .width(Length::Fill)
            .into()
    } else {
        measured_middle_ellipsized_text(
            entry.name().to_string_lossy().into_owned(),
            presentation.entry_text_size(),
        )
    };

    let trailing: Element<'static, Message> =
        if entry.kind == FileKind::Directory && !browser.is_trash_view {
            themed_icon(IconSymbol::ChevronRight, icon_tone, CHEVRON_ICON_SIZE).into()
        } else {
            Space::with_width(Length::Fixed(CHEVRON_ICON_SIZE)).into()
        };

    let row_content = row![
        entry_thumbnail_or_icon(browser, entry, icon_tone),
        name,
        trailing
    ]
    .spacing(presentation.entry_spacing())
    .align_items(Alignment::Center);
    let row_container = container(row_content)
        .padding(presentation.entry_padding())
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
        .on_enter(Message::EntryHovered(entry.path.clone()))
        .on_exit(Message::EntryHoverCleared(entry.path.clone()))
        .on_press(Message::ColumnEntryClicked(entry.path.clone()))
        .on_release(Message::EntryReleased)
        .on_right_press(Message::EntryRightClicked(entry.path.clone()))
        .interaction(iced::mouse::Interaction::Pointer);

    let row_area = if entry.kind == FileKind::Directory && !browser.is_trash_view {
        row_area.on_middle_press(Message::OpenDirectoryInNewTab(entry.path.clone()))
    } else {
        row_area
    };

    row_area.into()
}

fn is_drag_source(browser: &FileBrowser, path: &Path) -> bool {
    browser.file_drag.as_ref().is_some_and(|drag| {
        drag.is_dragging() && drag.sources.iter().any(|source| source.as_path() == path)
    })
}

fn column_content<'a>(browser: &'a FileBrowser, directory: &Path) -> ColumnContent<'a> {
    if directory == browser.current_dir {
        if browser.is_loading && browser.entries.is_empty() {
            return ColumnContent::Loading;
        }
        if browser.entries.is_empty() {
            return ColumnContent::Empty;
        }
        return ColumnContent::Entries(&browser.entries);
    }

    match browser.expanded_directories.get(directory) {
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
    if browser.is_trash_view {
        return vec![browser.current_dir.clone()];
    }

    if let Some(drag) = browser.file_drag.as_ref() {
        if !drag.column_directories_snapshot.is_empty() {
            return drag.column_directories_snapshot.clone();
        }
    }

    let mut directories = vec![browser.current_dir.clone()];
    let Some(selected) = browser.selected.as_ref() else {
        return directories;
    };

    let selected_directory = match selected_entry(browser).map(|entry| entry.kind) {
        Some(FileKind::Directory) => selected.clone(),
        _ => selected
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| browser.current_dir.clone()),
    };

    if selected_directory == browser.current_dir
        || !selected_directory.starts_with(&browser.current_dir)
    {
        return directories;
    }

    let mut ancestors = Vec::new();
    let mut cursor = Some(selected_directory.as_path());
    while let Some(path) = cursor {
        if path == browser.current_dir {
            break;
        }
        if !path.starts_with(&browser.current_dir) {
            break;
        }
        ancestors.push(path.to_path_buf());
        cursor = path.parent();
    }
    ancestors.reverse();
    directories.extend(ancestors);
    directories
}

fn column_presentation(
    column_view_mode: ColumnViewMode,
    index: usize,
    focused_index: usize,
) -> ColumnPresentation {
    match column_view_mode {
        ColumnViewMode::Unbounded if index == focused_index => ColumnPresentation::Focused,
        ColumnViewMode::Unbounded => ColumnPresentation::Compressed,
        ColumnViewMode::Fixed => ColumnPresentation::Balanced,
    }
}

#[derive(Debug, Clone, Copy)]
enum ColumnPresentation {
    Focused,
    Compressed,
    Balanced,
}

impl ColumnPresentation {
    fn width(self, browser: &FileBrowser) -> Length {
        match self {
            ColumnPresentation::Focused => Length::Fixed(browser.unbounded_column_width),
            ColumnPresentation::Compressed => Length::Fixed(COMPRESSED_UNBOUNDED_COLUMN_WIDTH),
            ColumnPresentation::Balanced => Length::Fixed(fixed_column_width(browser)),
        }
    }

    fn column_padding(self) -> [u16; 2] {
        match self {
            ColumnPresentation::Compressed => [8, 5],
            ColumnPresentation::Focused | ColumnPresentation::Balanced => [10, 8],
        }
    }

    fn content_spacing(self) -> u16 {
        match self {
            ColumnPresentation::Compressed => 3,
            ColumnPresentation::Focused | ColumnPresentation::Balanced => 4,
        }
    }

    fn title_padding(self) -> [u16; 4] {
        match self {
            ColumnPresentation::Compressed => [0, 5, 5, 5],
            ColumnPresentation::Focused | ColumnPresentation::Balanced => [0, 7, 6, 7],
        }
    }

    fn title_text_size(self) -> u16 {
        match self {
            ColumnPresentation::Compressed => 12,
            ColumnPresentation::Focused | ColumnPresentation::Balanced => 13,
        }
    }

    fn entry_padding(self) -> [u16; 2] {
        match self {
            ColumnPresentation::Compressed => [5, 5],
            ColumnPresentation::Focused | ColumnPresentation::Balanced => [6, 8],
        }
    }

    fn entry_spacing(self) -> u16 {
        match self {
            ColumnPresentation::Compressed => 5,
            ColumnPresentation::Focused | ColumnPresentation::Balanced => 8,
        }
    }

    fn entry_text_size(self) -> u16 {
        match self {
            ColumnPresentation::Compressed => 12,
            ColumnPresentation::Focused | ColumnPresentation::Balanced => 16,
        }
    }
}

fn fixed_column_width(browser: &FileBrowser) -> f32 {
    let fixed_count = browser.column_fixed_count.max(1) as f32;
    let browser_width = (browser.main_window_width - SIDEBAR_WIDTH).max(MIN_FIXED_COLUMN_WIDTH);
    (browser_width / fixed_count).max(MIN_FIXED_COLUMN_WIDTH)
}

fn active_child_for_column(
    browser: &FileBrowser,
    directory: &Path,
    next_directory: Option<&PathBuf>,
) -> Option<PathBuf> {
    if let Some(next_directory) = next_directory {
        return Some(next_directory.clone());
    }

    let selected = browser.selected.as_ref()?;
    if selected.parent() == Some(directory) {
        return Some(selected.clone());
    }
    None
}

fn selected_entry(browser: &FileBrowser) -> Option<&DirectoryEntry> {
    let selected = browser.selected.as_deref()?;
    find_entry(&browser.entries, &browser.expanded_directories, selected)
}

fn find_entry<'a>(
    entries: &'a [DirectoryEntry],
    expanded_directories: &'a std::collections::HashMap<PathBuf, crate::model::ExpandedDirectory>,
    path: &Path,
) -> Option<&'a DirectoryEntry> {
    for entry in entries {
        if entry.path == path {
            return Some(entry);
        }
        let Some(expanded) = expanded_directories.get(&entry.path) else {
            continue;
        };
        if matches!(expanded.status, ExpandedDirectoryStatus::Loaded) {
            if let Some(child) = find_entry(&expanded.entries, expanded_directories, path) {
                return Some(child);
            }
        }
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
        .center_x()
        .center_y()
        .into()
}

fn entry_icon(entry: &DirectoryEntry, tone: IconTone) -> Svg<Theme> {
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

fn column_scroll_id(directory: &Path) -> scrollable::Id {
    scrollable::Id::new(format!("column-scroll-{}", path_hash(directory)))
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

fn themed_icon(symbol: IconSymbol, tone: IconTone, size: f32) -> Svg<Theme> {
    symbol.view(size).style(icon_tone_style(tone))
}

fn icon_tone_style(tone: IconTone) -> iced::theme::Svg {
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
