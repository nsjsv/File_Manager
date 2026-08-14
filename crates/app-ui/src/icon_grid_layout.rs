use std::collections::HashMap;
use std::path::{Path, PathBuf};

use file_core::DirectoryEntry;

use crate::icon_grid_geometry::{
    column_count_for_width, keyboard_target_index, row_count_for_entries, row_height,
    tile_visual_height, tile_width, IconGridDirection, ICON_GRID_CONTENT_PADDING, ICON_GRID_GAP,
    ICON_GRID_OVERSCAN_ROWS,
};
use crate::model::{
    ExpandedDirectoryStatus, IconGridExpandedDirectory, IconGridExpansionState, IconGridViewport,
};
use crate::virtual_range::vertical_scroll_delta_to_reveal;

const ICON_GRID_INITIAL_ROWS: usize = ICON_GRID_OVERSCAN_ROWS * 2 + 1;
pub(crate) const ICON_GRID_STATUS_HEIGHT: f32 = 48.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IconGridPanelStatus {
    Loaded,
    Loading,
    Empty,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct IconGridVisibleRows {
    pub(crate) start_row: usize,
    pub(crate) end_row: usize,
    pub(crate) before_height: f32,
    pub(crate) after_height: f32,
}

#[derive(Debug)]
pub(crate) struct IconGridRowsLayout<'a> {
    pub(crate) directory: &'a Path,
    pub(crate) entries: &'a [DirectoryEntry],
    pub(crate) start_row: usize,
    pub(crate) end_row: usize,
    pub(crate) column_count: usize,
    pub(crate) top: f32,
    pub(crate) height: f32,
}

impl IconGridRowsLayout<'_> {
    pub(crate) fn visible_rows(
        &self,
        panel_top: f32,
        clip_bottom: f32,
        viewport: IconGridViewport,
        icon_edge: u32,
    ) -> Option<IconGridVisibleRows> {
        let (visible_top, visible_bottom) = visible_vertical_window(viewport, icon_edge);
        self.visible_rows_in_window(
            panel_top,
            clip_bottom,
            visible_top,
            visible_bottom,
            icon_edge,
        )
    }

    fn visible_rows_in_window(
        &self,
        panel_top: f32,
        clip_bottom: f32,
        visible_top: f32,
        visible_bottom: f32,
        icon_edge: u32,
    ) -> Option<IconGridVisibleRows> {
        let rows_top = panel_top + self.top;
        let rows_bottom = (rows_top + self.height).min(clip_bottom);
        let intersection_top = rows_top.max(visible_top);
        let intersection_bottom = rows_bottom.min(visible_bottom);
        if intersection_top >= intersection_bottom {
            return None;
        }

        let row_height = row_height(icon_edge);
        let local_start = ((intersection_top - rows_top) / row_height)
            .floor()
            .max(0.0) as usize;
        let local_end = ((intersection_bottom - rows_top) / row_height)
            .ceil()
            .max(0.0) as usize;
        let start_row = self.start_row.saturating_add(local_start).min(self.end_row);
        let end_row = self.start_row.saturating_add(local_end).min(self.end_row);
        Some(IconGridVisibleRows {
            start_row,
            end_row,
            before_height: start_row.saturating_sub(self.start_row) as f32 * row_height,
            after_height: self.end_row.saturating_sub(end_row) as f32 * row_height,
        })
    }
}

#[derive(Debug)]
pub(crate) struct IconGridBandLayout<'a> {
    pub(crate) directory: &'a Path,
    pub(crate) anchor_column: usize,
    pub(crate) top: f32,
    pub(crate) height: f32,
    pub(crate) natural_height: f32,
    pub(crate) interactive: bool,
    pub(crate) panel: Box<IconGridPanelLayout<'a>>,
}

#[derive(Debug)]
pub(crate) enum IconGridFlowSegment<'a> {
    Rows(IconGridRowsLayout<'a>),
    Band(IconGridBandLayout<'a>),
}

impl IconGridFlowSegment<'_> {
    pub(crate) fn top(&self) -> f32 {
        match self {
            Self::Rows(rows) => rows.top,
            Self::Band(band) => band.top,
        }
    }

    pub(crate) fn height(&self) -> f32 {
        match self {
            Self::Rows(rows) => rows.height,
            Self::Band(band) => band.height,
        }
    }
}

#[derive(Debug)]
pub(crate) struct IconGridPanelLayout<'a> {
    pub(crate) status: IconGridPanelStatus,
    pub(crate) height: f32,
    pub(crate) flow: Vec<IconGridFlowSegment<'a>>,
}

#[derive(Debug)]
pub(crate) struct IconGridLayout<'a> {
    icon_edge: u32,
    root: IconGridPanelLayout<'a>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct IconGridVisibleEntry<'a> {
    pub(crate) entry: &'a DirectoryEntry,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct IconGridNavigationTarget<'a> {
    pub(crate) directory: &'a Path,
    pub(crate) entry: &'a DirectoryEntry,
}

#[derive(Debug, Clone, Copy)]
struct IconGridEntryGeometry<'a> {
    directory: &'a Path,
    entry: &'a DirectoryEntry,
    center_x: f32,
    center_y: f32,
    top: f32,
    bottom: f32,
}

impl<'a> IconGridLayout<'a> {
    pub(crate) fn new(
        root_directory: &'a Path,
        root_entries: &'a [DirectoryEntry],
        viewport_width: f32,
        icon_edge: u32,
        expansion: Option<&'a IconGridExpansionState>,
    ) -> Self {
        let expansion = expansion.filter(|state| state.context().current_dir == root_directory);
        let children_by_parent = expansion.map(index_visible_children);
        let root_status = if root_entries.is_empty() {
            IconGridPanelStatus::Empty
        } else {
            IconGridPanelStatus::Loaded
        };
        let root = build_panel(
            root_directory,
            root_entries,
            root_status,
            viewport_width.max(0.0),
            icon_edge,
            children_by_parent.as_ref(),
        );
        Self { icon_edge, root }
    }

    pub(crate) fn root(&self) -> &IconGridPanelLayout<'a> {
        &self.root
    }

    pub(crate) fn total_height(&self) -> f32 {
        self.root.height
    }

    pub(crate) fn visible_entries(
        &self,
        viewport: IconGridViewport,
    ) -> Vec<IconGridVisibleEntry<'a>> {
        let (visible_top, visible_bottom) = visible_vertical_window(viewport, self.icon_edge);
        let mut entries = Vec::new();
        collect_visible_entries(
            &self.root,
            0.0,
            self.root.height,
            visible_top,
            visible_bottom,
            self.icon_edge,
            &mut entries,
        );
        entries
    }

    pub(crate) fn interactive_entry_paths(&self) -> Vec<PathBuf> {
        self.interactive_entry_geometry()
            .into_iter()
            .map(|entry| entry.entry.path.clone())
            .collect()
    }

    pub(crate) fn keyboard_target(
        &self,
        current_path: Option<&Path>,
        direction: IconGridDirection,
    ) -> Option<IconGridNavigationTarget<'a>> {
        if let [IconGridFlowSegment::Rows(rows)] = self.root.flow.as_slice() {
            let current_index = current_path.and_then(|current_path| {
                rows.entries
                    .iter()
                    .position(|entry| entry.path == current_path)
            });
            let target_index = keyboard_target_index(
                current_index,
                direction,
                rows.entries.len(),
                rows.column_count,
            )?;
            return Some(IconGridNavigationTarget {
                directory: rows.directory,
                entry: &rows.entries[target_index],
            });
        }

        let entries = self.interactive_entry_geometry();
        if entries.is_empty() {
            return None;
        }

        let current_index = current_path.and_then(|current_path| {
            entries
                .iter()
                .position(|entry| entry.entry.path == current_path)
        });
        let target = match current_index {
            None => match direction {
                IconGridDirection::Up | IconGridDirection::Left => *entries.last()?,
                IconGridDirection::Down | IconGridDirection::Right => *entries.first()?,
            },
            Some(index) => match direction {
                IconGridDirection::Left => entries[index.saturating_sub(1)],
                IconGridDirection::Right => entries[(index + 1).min(entries.len() - 1)],
                IconGridDirection::Up => adjacent_vertical_entry(&entries, index, false),
                IconGridDirection::Down => adjacent_vertical_entry(&entries, index, true),
            },
        };

        Some(IconGridNavigationTarget {
            directory: target.directory,
            entry: target.entry,
        })
    }

    pub(crate) fn scroll_delta_to_reveal(&self, viewport: IconGridViewport, path: &Path) -> f32 {
        let Some(entry) = find_interactive_entry(&self.root, 0.0, 0.0, self.icon_edge, path) else {
            return 0.0;
        };
        vertical_scroll_delta_to_reveal(
            viewport.offset_y,
            viewport.height,
            entry.top,
            entry.bottom - entry.top,
        )
    }

    fn interactive_entry_geometry(&self) -> Vec<IconGridEntryGeometry<'a>> {
        let mut entries = Vec::new();
        collect_interactive_entries(&self.root, 0.0, 0.0, self.icon_edge, &mut entries);
        entries
    }
}

fn build_panel<'a>(
    directory: &'a Path,
    entries: &'a [DirectoryEntry],
    status: IconGridPanelStatus,
    width: f32,
    icon_edge: u32,
    children_by_parent: Option<&IconGridChildrenByParent<'a>>,
) -> IconGridPanelLayout<'a> {
    let column_count = column_count_for_width(width, icon_edge);
    if status != IconGridPanelStatus::Loaded || entries.is_empty() {
        return IconGridPanelLayout {
            status,
            height: ICON_GRID_CONTENT_PADDING * 2.0 + ICON_GRID_STATUS_HEIGHT,
            flow: Vec::new(),
        };
    }

    let total_rows = row_count_for_entries(entries.len(), column_count);
    let children = children_by_parent
        .and_then(|children| children.get(directory))
        .map(Vec::as_slice)
        .unwrap_or_default();

    let mut flow = Vec::with_capacity(children.len().saturating_mul(2).saturating_add(1));
    let mut next_row = 0;
    let mut top = ICON_GRID_CONTENT_PADDING;
    let mut child_cursor = 0;
    while child_cursor < children.len() {
        let anchor_row = children[child_cursor].1.anchor_index / column_count;
        let rows_end = anchor_row.saturating_add(1).min(total_rows);
        if next_row < rows_end {
            let height = (rows_end - next_row) as f32 * row_height(icon_edge);
            flow.push(IconGridFlowSegment::Rows(IconGridRowsLayout {
                directory,
                entries,
                start_row: next_row,
                end_row: rows_end,
                column_count,
                top,
                height,
            }));
            top += height;
            next_row = rows_end;
        }

        while child_cursor < children.len()
            && children[child_cursor].1.anchor_index / column_count == anchor_row
        {
            let (child_path, child) = children[child_cursor];
            let (child_entries, child_status) = expanded_panel_content(child);
            let child_panel = build_panel(
                child_path,
                child_entries,
                child_status,
                width,
                icon_edge,
                children_by_parent,
            );
            let natural_height = child_panel.height;
            let animation_progress = child.contents.animation_progress.clamp(0.0, 1.0);
            let height = natural_height * animation_progress;
            flow.push(IconGridFlowSegment::Band(IconGridBandLayout {
                directory: child_path,
                anchor_column: child.anchor_index % column_count,
                top,
                height,
                natural_height,
                interactive: child.is_interactive(),
                panel: Box::new(child_panel),
            }));
            top += height;
            child_cursor += 1;
        }
    }

    if next_row < total_rows {
        let height = (total_rows - next_row) as f32 * row_height(icon_edge);
        flow.push(IconGridFlowSegment::Rows(IconGridRowsLayout {
            directory,
            entries,
            start_row: next_row,
            end_row: total_rows,
            column_count,
            top,
            height,
        }));
        top += height;
    }

    IconGridPanelLayout {
        status,
        height: top + ICON_GRID_CONTENT_PADDING,
        flow,
    }
}

type IconGridChildrenByParent<'a> =
    HashMap<&'a Path, Vec<(&'a Path, &'a IconGridExpandedDirectory)>>;

fn index_visible_children(expansion: &IconGridExpansionState) -> IconGridChildrenByParent<'_> {
    let mut children_by_parent = IconGridChildrenByParent::new();
    for (path, directory) in expansion
        .directories()
        .filter(|(_, directory)| directory.is_visible())
    {
        children_by_parent
            .entry(directory.parent_directory.as_path())
            .or_default()
            .push((path, directory));
    }
    for children in children_by_parent.values_mut() {
        children.sort_by(|(left_path, left), (right_path, right)| {
            left.anchor_index
                .cmp(&right.anchor_index)
                .then_with(|| left_path.cmp(right_path))
        });
    }
    children_by_parent
}

fn expanded_panel_content(
    directory: &IconGridExpandedDirectory,
) -> (&[DirectoryEntry], IconGridPanelStatus) {
    match &directory.contents.status {
        ExpandedDirectoryStatus::Loading => (&[], IconGridPanelStatus::Loading),
        ExpandedDirectoryStatus::Loaded if directory.contents.entries.is_empty() => {
            (&directory.contents.entries, IconGridPanelStatus::Empty)
        }
        ExpandedDirectoryStatus::Loaded => {
            (&directory.contents.entries, IconGridPanelStatus::Loaded)
        }
        ExpandedDirectoryStatus::Error => (&[], IconGridPanelStatus::Error),
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_visible_entries<'a>(
    panel: &IconGridPanelLayout<'a>,
    panel_top: f32,
    clip_bottom: f32,
    visible_top: f32,
    visible_bottom: f32,
    icon_edge: u32,
    collected: &mut Vec<IconGridVisibleEntry<'a>>,
) {
    for segment in &panel.flow {
        match segment {
            IconGridFlowSegment::Rows(rows) => {
                let Some(visible_rows) = rows.visible_rows_in_window(
                    panel_top,
                    clip_bottom,
                    visible_top,
                    visible_bottom,
                    icon_edge,
                ) else {
                    continue;
                };
                for row in visible_rows.start_row..visible_rows.end_row {
                    let start = row
                        .saturating_mul(rows.column_count)
                        .min(rows.entries.len());
                    let end = start
                        .saturating_add(rows.column_count)
                        .min(rows.entries.len());
                    for entry in &rows.entries[start..end] {
                        collected.push(IconGridVisibleEntry { entry });
                    }
                }
            }
            IconGridFlowSegment::Band(band) if band.interactive => {
                let band_top = panel_top + band.top;
                let band_bottom = (band_top + band.height).min(clip_bottom);
                if band_top >= band_bottom
                    || band_top >= visible_bottom
                    || band_bottom <= visible_top
                {
                    continue;
                }
                collect_visible_entries(
                    &band.panel,
                    band_top,
                    band_bottom,
                    visible_top,
                    visible_bottom,
                    icon_edge,
                    collected,
                );
            }
            IconGridFlowSegment::Band(_) => {}
        }
    }
}

fn collect_interactive_entries<'a>(
    panel: &IconGridPanelLayout<'a>,
    panel_top: f32,
    panel_left: f32,
    icon_edge: u32,
    collected: &mut Vec<IconGridEntryGeometry<'a>>,
) {
    for segment in &panel.flow {
        match segment {
            IconGridFlowSegment::Rows(rows) => {
                let rows_top = panel_top + rows.top;
                for row in rows.start_row..rows.end_row {
                    let start = row
                        .saturating_mul(rows.column_count)
                        .min(rows.entries.len());
                    let end = start
                        .saturating_add(rows.column_count)
                        .min(rows.entries.len());
                    let top = rows_top
                        + row.saturating_sub(rows.start_row) as f32 * row_height(icon_edge);
                    for (column, entry) in rows.entries[start..end].iter().enumerate() {
                        collected.push(IconGridEntryGeometry {
                            directory: rows.directory,
                            entry,
                            center_x: panel_left
                                + ICON_GRID_CONTENT_PADDING
                                + column as f32 * (tile_width(icon_edge) + ICON_GRID_GAP)
                                + tile_width(icon_edge) / 2.0,
                            center_y: top + tile_visual_height(icon_edge) / 2.0,
                            top,
                            bottom: top + tile_visual_height(icon_edge),
                        });
                    }
                }
            }
            IconGridFlowSegment::Band(band) if band.interactive => {
                collect_interactive_entries(
                    &band.panel,
                    panel_top + band.top,
                    panel_left,
                    icon_edge,
                    collected,
                );
            }
            IconGridFlowSegment::Band(_) => {}
        }
    }
}

fn find_interactive_entry<'a>(
    panel: &IconGridPanelLayout<'a>,
    panel_top: f32,
    panel_left: f32,
    icon_edge: u32,
    path: &Path,
) -> Option<IconGridEntryGeometry<'a>> {
    for segment in &panel.flow {
        match segment {
            IconGridFlowSegment::Rows(rows) => {
                let start = rows
                    .start_row
                    .saturating_mul(rows.column_count)
                    .min(rows.entries.len());
                let end = rows
                    .end_row
                    .saturating_mul(rows.column_count)
                    .min(rows.entries.len());
                let Some(relative_index) = rows.entries[start..end]
                    .iter()
                    .position(|entry| entry.path == path)
                else {
                    continue;
                };
                let index = relative_index + start;
                let row = index / rows.column_count;
                let column = index % rows.column_count;
                let top = panel_top
                    + rows.top
                    + row.saturating_sub(rows.start_row) as f32 * row_height(icon_edge);
                return Some(IconGridEntryGeometry {
                    directory: rows.directory,
                    entry: &rows.entries[index],
                    center_x: panel_left
                        + ICON_GRID_CONTENT_PADDING
                        + column as f32 * (tile_width(icon_edge) + ICON_GRID_GAP)
                        + tile_width(icon_edge) / 2.0,
                    center_y: top + tile_visual_height(icon_edge) / 2.0,
                    top,
                    bottom: top + tile_visual_height(icon_edge),
                });
            }
            IconGridFlowSegment::Band(band) if band.interactive => {
                if let Some(entry) = find_interactive_entry(
                    &band.panel,
                    panel_top + band.top,
                    panel_left,
                    icon_edge,
                    path,
                ) {
                    return Some(entry);
                }
            }
            IconGridFlowSegment::Band(_) => {}
        }
    }
    None
}

fn visible_vertical_window(viewport: IconGridViewport, icon_edge: u32) -> (f32, f32) {
    if viewport.width > f32::EPSILON && viewport.height > f32::EPSILON {
        let overscan = ICON_GRID_OVERSCAN_ROWS as f32 * row_height(icon_edge);
        (
            (viewport.offset_y - overscan).max(0.0),
            viewport.offset_y + viewport.height + overscan,
        )
    } else {
        (
            0.0,
            ICON_GRID_CONTENT_PADDING + ICON_GRID_INITIAL_ROWS as f32 * row_height(icon_edge),
        )
    }
}

fn adjacent_vertical_entry<'a>(
    entries: &[IconGridEntryGeometry<'a>],
    current_index: usize,
    move_down: bool,
) -> IconGridEntryGeometry<'a> {
    let current = entries[current_index];
    let target_y = entries
        .iter()
        .filter(|entry| {
            if move_down {
                entry.center_y > current.center_y + f32::EPSILON
            } else {
                entry.center_y < current.center_y - f32::EPSILON
            }
        })
        .map(|entry| entry.center_y)
        .reduce(|candidate, y| {
            if move_down {
                candidate.min(y)
            } else {
                candidate.max(y)
            }
        });
    let Some(target_y) = target_y else {
        return current;
    };

    entries
        .iter()
        .filter(|entry| (entry.center_y - target_y).abs() <= f32::EPSILON)
        .min_by(|left, right| {
            (left.center_x - current.center_x)
                .abs()
                .total_cmp(&(right.center_x - current.center_x).abs())
                .then_with(|| left.center_x.total_cmp(&right.center_x))
        })
        .copied()
        .unwrap_or(current)
}

#[cfg(test)]
#[path = "icon_grid_layout_tests.rs"]
mod tests;
