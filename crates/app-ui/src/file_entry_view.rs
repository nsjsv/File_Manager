use std::path::Path;

use file_core::{DirectoryEntry, FileKind};
use iced::widget::{container, image, Svg};
use iced::{Element, Length, Theme};

use crate::app::panes::BrowserPaneView;
use crate::app::FileBrowser;
use crate::appearance::{
    dragged_row_style, hovered_row_style, icon_svg_style, muted_icon_svg_style,
    open_child_row_style, selected_icon_svg_style, selected_row_style, selected_row_style_for_run,
    warning_icon_svg_style,
};
use crate::file_entry_presentation::SelectionRunPosition;
use crate::icons::{file_entry_icon_symbol, IconSymbol};
use crate::model::Message;
use crate::thumbnail_cache::{
    COLUMN_THUMBNAIL_EDGE, COLUMN_THUMBNAIL_SIZE, LIST_THUMBNAIL_EDGE, LIST_THUMBNAIL_SIZE,
};

pub(crate) const ENTRY_ICON_SIZE: f32 = 18.0;
const COLUMN_ENTRY_ICON_SIZE: f32 = 16.0;

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileEntryIconDensity {
    List,
    Column,
    Grid(u32),
}

impl FileEntryIconDensity {
    fn thumbnail_edge(self) -> u32 {
        match self {
            Self::List => LIST_THUMBNAIL_EDGE,
            Self::Column => COLUMN_THUMBNAIL_EDGE,
            Self::Grid(icon_edge) => crate::icon_grid_geometry::thumbnail_edge(icon_edge),
        }
    }

    fn thumbnail_size(self) -> f32 {
        match self {
            Self::List => LIST_THUMBNAIL_SIZE,
            Self::Column => COLUMN_THUMBNAIL_SIZE,
            Self::Grid(icon_edge) => icon_edge as f32,
        }
    }

    fn icon_size(self) -> f32 {
        match self {
            Self::List => ENTRY_ICON_SIZE,
            Self::Column => COLUMN_ENTRY_ICON_SIZE,
            Self::Grid(icon_edge) => icon_edge as f32 * 0.68,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileEntryVisualState {
    Normal,
    Hovered,
    OpenChild,
    Selected,
    Dragged,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileEntryIconTone {
    Normal,
    Selected,
    Muted,
    Warning,
}

impl FileEntryVisualState {
    pub(crate) fn from_entry_context(
        pane: BrowserPaneView<'_>,
        path: &Path,
        is_open_child: bool,
    ) -> Self {
        if is_drag_source(pane, path) {
            Self::Dragged
        } else if pane.is_path_selected(path) {
            Self::Selected
        } else if pane
            .hovered_entry
            .is_some_and(|hovered| hovered.as_path() == path)
        {
            Self::Hovered
        } else if is_open_child {
            Self::OpenChild
        } else {
            Self::Normal
        }
    }

    pub(crate) fn icon_tone(self) -> FileEntryIconTone {
        match self {
            Self::Dragged => FileEntryIconTone::Muted,
            Self::Selected => FileEntryIconTone::Selected,
            Self::Normal | Self::Hovered | Self::OpenChild => FileEntryIconTone::Normal,
        }
    }

    pub(crate) fn row_style_for_selection_run(
        self,
        selection_run_position: Option<SelectionRunPosition>,
    ) -> Option<Box<dyn Fn(&Theme) -> iced::widget::container::Style>> {
        match self {
            Self::Dragged => Some(Box::new(dragged_row_style)),
            Self::Selected => {
                let style: Box<dyn Fn(&Theme) -> iced::widget::container::Style> =
                    match selection_run_position {
                        Some(position) => Box::new(selected_row_style_for_run(position)),
                        None => Box::new(selected_row_style),
                    };
                Some(style)
            }
            Self::Hovered => Some(Box::new(hovered_row_style)),
            Self::OpenChild => Some(Box::new(open_child_row_style)),
            Self::Normal => None,
        }
    }
}

pub(crate) fn entry_thumbnail_or_icon<'a>(
    browser: &'a FileBrowser,
    entry: &DirectoryEntry,
    tone: FileEntryIconTone,
    density: FileEntryIconDensity,
) -> Element<'a, Message> {
    let thumbnail_edge = density.thumbnail_edge();
    let thumbnail_size = density.thumbnail_size();
    if let Some(thumbnail) = browser
        .thumbnail_cache
        .ready_for_entry(entry, thumbnail_edge)
    {
        return container(
            image::Image::new(thumbnail.handle.clone())
                .width(Length::Fixed(thumbnail_size))
                .height(Length::Fixed(thumbnail_size)),
        )
        .width(Length::Fixed(thumbnail_size))
        .height(Length::Fixed(thumbnail_size))
        .into();
    }

    if !thumbnails::is_supported_thumbnail_path(&entry.path) {
        return entry_icon(entry, tone, density).into();
    }

    container(entry_icon(entry, tone, density))
        .width(Length::Fixed(thumbnail_size))
        .height(Length::Fixed(thumbnail_size))
        .center_x(Length::Fixed(thumbnail_size))
        .center_y(Length::Fixed(thumbnail_size))
        .into()
}

pub(crate) fn themed_icon(
    symbol: IconSymbol,
    tone: FileEntryIconTone,
    size: f32,
) -> Svg<'static, Theme> {
    symbol.view(size).style(icon_tone_style(tone))
}

fn entry_icon(
    entry: &DirectoryEntry,
    tone: FileEntryIconTone,
    density: FileEntryIconDensity,
) -> Svg<'static, Theme> {
    let symbol = if entry.kind == FileKind::Symlink && entry.is_broken_symlink {
        IconSymbol::TriangleAlert
    } else {
        file_entry_icon_symbol(entry.kind, entry.name())
    };
    let tone = match (symbol, tone) {
        (IconSymbol::TriangleAlert, FileEntryIconTone::Muted) => FileEntryIconTone::Muted,
        (IconSymbol::TriangleAlert, _) => FileEntryIconTone::Warning,
        _ => tone,
    };
    themed_icon(symbol, tone, density.icon_size())
}

fn icon_tone_style(
    tone: FileEntryIconTone,
) -> fn(&Theme, iced::widget::svg::Status) -> iced::widget::svg::Style {
    match tone {
        FileEntryIconTone::Normal => icon_svg_style(),
        FileEntryIconTone::Selected => selected_icon_svg_style(),
        FileEntryIconTone::Muted => muted_icon_svg_style(),
        FileEntryIconTone::Warning => warning_icon_svg_style(),
    }
}

fn is_drag_source(pane: BrowserPaneView<'_>, path: &Path) -> bool {
    pane.file_drag.is_some_and(|drag| {
        drag.is_dragging() && drag.sources.iter().any(|source| source.as_path() == path)
    })
}
