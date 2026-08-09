use std::path::PathBuf;

use super::*;
use crate::icon_grid_geometry::visible_entry_range;
use crate::model::{
    BrowserPaneId, ExpandedDirectory, IconGridExpansionAnchor, IconGridExpansionContext,
    IconGridExpansionSessionId,
};
use file_core::{EntryMetadata, FileKind};
use tokio_util::sync::CancellationToken;

fn entry(path: impl Into<PathBuf>, kind: FileKind) -> DirectoryEntry {
    DirectoryEntry::new(
        path.into(),
        kind,
        EntryMetadata::default(),
        false,
        false,
        false,
    )
}

fn files(directory: &str, count: usize) -> Vec<DirectoryEntry> {
    (0..count)
        .map(|index| entry(format!("{directory}/item-{index:03}"), FileKind::File))
        .collect()
}

fn loaded(entries: Vec<DirectoryEntry>) -> ExpandedDirectory {
    ExpandedDirectory {
        entries,
        directory_discovery: None,
        status: ExpandedDirectoryStatus::Loaded,
        is_expanded: true,
        is_collapsing: false,
        animation_progress: 1.0,
        load_generation: 0,
        load_context: None,
        load_cancel: Some(CancellationToken::new()),
        directory_order_phase: crate::model::DirectoryOrderPhase::Ready {
            field: file_core::SortField::Name,
            direction: file_core::SortDirection::Ascending,
        },
    }
}

fn anchor(parent: &str, path: &str, index: usize) -> IconGridExpansionAnchor {
    IconGridExpansionAnchor {
        parent_directory: PathBuf::from(parent),
        path: PathBuf::from(path),
        index,
    }
}

fn expansion(root_entries: Vec<DirectoryEntry>) -> IconGridExpansionState {
    IconGridExpansionState::new(
        IconGridExpansionContext {
            pane_id: BrowserPaneId(1),
            current_dir: PathBuf::from("/workspace"),
            session_id: IconGridExpansionSessionId::new(1),
        },
        anchor("/workspace", "/workspace/root", 0),
        loaded(root_entries),
    )
}

fn first_band<'a>(layout: &'a IconGridLayout<'a>) -> &'a IconGridBandLayout<'a> {
    layout
        .root()
        .flow
        .iter()
        .find_map(|segment| match segment {
            IconGridFlowSegment::Band(band) => Some(band),
            IconGridFlowSegment::Rows(_) => None,
        })
        .expect("layout should include expansion band")
}

#[test]
fn flat_layout_matches_existing_virtual_range() {
    let entries = files("/workspace", 44);
    let viewport = IconGridViewport {
        offset_y: ICON_GRID_CONTENT_PADDING + row_height(96) * 10.0,
        width: 500.0,
        height: row_height(96) * 2.0,
    };
    let old_range = visible_entry_range(viewport, entries.len(), 96);
    let layout = IconGridLayout::new(Path::new("/workspace"), &entries, 500.0, 96, None);
    let visible = layout.visible_entries(viewport);

    assert_eq!(layout.root().flow.len(), 1);
    assert_eq!(visible.len(), old_range.end_entry - old_range.start_entry);
    assert_eq!(
        visible.first().unwrap().entry.path,
        entries[old_range.start_entry].path
    );
    assert_eq!(
        visible.last().unwrap().entry.path,
        entries[old_range.end_entry - 1].path
    );
    assert_eq!(
        layout.total_height(),
        ICON_GRID_CONTENT_PADDING * 2.0
            + row_count_for_entries(entries.len(), 3) as f32 * row_height(96)
    );
}

#[test]
fn nested_panel_keeps_the_full_width_column_count() {
    let root_entries = vec![
        entry("/workspace/root/child", FileKind::Directory),
        entry("/workspace/root/file-a", FileKind::File),
        entry("/workspace/root/file-b", FileKind::File),
    ];
    let state = expansion(root_entries);
    let root_entries = vec![entry("/workspace/root", FileKind::Directory)];
    let layout = IconGridLayout::new(
        Path::new("/workspace"),
        &root_entries,
        440.0,
        96,
        Some(&state),
    );
    let band = first_band(&layout);
    let nested_columns = band
        .panel
        .flow
        .iter()
        .find_map(|segment| match segment {
            IconGridFlowSegment::Rows(rows) => Some(rows.column_count),
            IconGridFlowSegment::Band(_) => None,
        })
        .unwrap();

    assert_eq!(nested_columns, column_count_for_width(440.0, 96));
}

#[test]
fn opening_band_does_not_schedule_thumbnail_entries() {
    let child = entry("/workspace/root/child.png", FileKind::File);
    let mut opening = loaded(vec![child.clone()]);
    opening.animation_progress = 0.5;
    let state = IconGridExpansionState::new(
        IconGridExpansionContext {
            pane_id: BrowserPaneId(1),
            current_dir: PathBuf::from("/workspace"),
            session_id: IconGridExpansionSessionId::new(1),
        },
        anchor("/workspace", "/workspace/root", 0),
        opening,
    );
    let root_entries = vec![entry("/workspace/root", FileKind::Directory)];
    let layout = IconGridLayout::new(
        Path::new("/workspace"),
        &root_entries,
        500.0,
        96,
        Some(&state),
    );
    let visible = layout.visible_entries(IconGridViewport {
        offset_y: 0.0,
        width: 500.0,
        height: 800.0,
    });

    assert!(visible
        .iter()
        .all(|visible| visible.entry.path != child.path));
}

#[test]
fn root_band_is_inserted_after_its_complete_visual_row() {
    let root_entries = vec![
        entry("/workspace/a", FileKind::File),
        entry("/workspace/root", FileKind::Directory),
        entry("/workspace/c", FileKind::File),
        entry("/workspace/d", FileKind::File),
    ];
    let state = IconGridExpansionState::new(
        IconGridExpansionContext {
            pane_id: BrowserPaneId(1),
            current_dir: PathBuf::from("/workspace"),
            session_id: IconGridExpansionSessionId::new(1),
        },
        anchor("/workspace", "/workspace/root", 1),
        loaded(files("/workspace/root", 2)),
    );
    let layout = IconGridLayout::new(
        Path::new("/workspace"),
        &root_entries,
        500.0,
        96,
        Some(&state),
    );
    let band = first_band(&layout);

    assert_eq!(band.top, ICON_GRID_CONTENT_PADDING + row_height(96));
    assert_eq!(band.anchor_column, 1);
    let rows_after = layout
        .root()
        .flow
        .iter()
        .find_map(|segment| match segment {
            IconGridFlowSegment::Rows(rows) if rows.start_row == 1 => Some(rows),
            _ => None,
        })
        .unwrap();
    assert_eq!(rows_after.top, band.top + band.height);
}

#[test]
fn panel_layout_contains_only_the_active_sibling_band() {
    let root_entries = vec![
        entry("/workspace/alpha", FileKind::Directory),
        entry("/workspace/beta", FileKind::Directory),
        entry("/workspace/tail", FileKind::File),
    ];
    let mut state = IconGridExpansionState::new(
        IconGridExpansionContext {
            pane_id: BrowserPaneId(1),
            current_dir: PathBuf::from("/workspace"),
            session_id: IconGridExpansionSessionId::new(1),
        },
        anchor("/workspace", "/workspace/alpha", 0),
        loaded(files("/workspace/alpha", 1)),
    );
    assert!(!state.insert_directory(
        anchor("/workspace", "/workspace/beta", 1),
        loaded(files("/workspace/beta", 2)),
    ));
    let layout = IconGridLayout::new(
        Path::new("/workspace"),
        &root_entries,
        500.0,
        96,
        Some(&state),
    );
    let bands = layout
        .root()
        .flow
        .iter()
        .filter_map(|segment| match segment {
            IconGridFlowSegment::Band(band) => Some(band),
            IconGridFlowSegment::Rows(_) => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(bands.len(), 1);
    assert_eq!(bands[0].directory, Path::new("/workspace/alpha"));
}

#[test]
fn nested_band_contributes_to_parent_natural_height() {
    let root_entries = vec![entry("/workspace/root", FileKind::Directory)];
    let mut state = IconGridExpansionState::new(
        IconGridExpansionContext {
            pane_id: BrowserPaneId(1),
            current_dir: PathBuf::from("/workspace"),
            session_id: IconGridExpansionSessionId::new(1),
        },
        anchor("/workspace", "/workspace/root", 0),
        loaded(vec![entry("/workspace/root/nested", FileKind::Directory)]),
    );
    assert!(state.insert_directory(
        anchor("/workspace/root", "/workspace/root/nested", 0,),
        loaded(files("/workspace/root/nested", 2)),
    ));
    let layout = IconGridLayout::new(
        Path::new("/workspace"),
        &root_entries,
        500.0,
        96,
        Some(&state),
    );
    let root_band = first_band(&layout);
    let nested_band = root_band
        .panel
        .flow
        .iter()
        .find_map(|segment| match segment {
            IconGridFlowSegment::Band(band) => Some(band),
            IconGridFlowSegment::Rows(_) => None,
        })
        .unwrap();

    assert_eq!(root_band.height, root_band.natural_height);
    assert!(root_band.natural_height > nested_band.natural_height);
}

#[test]
fn collapse_fraction_clips_band_height_without_flattening_entries() {
    let root_entries = vec![entry("/workspace/root", FileKind::Directory)];
    let mut state = expansion(files("/workspace/root", 20));
    let root = state.directory_mut(Path::new("/workspace/root")).unwrap();
    root.contents.is_expanded = false;
    root.contents.is_collapsing = true;
    root.contents.animation_progress = 0.5;
    let layout = IconGridLayout::new(
        Path::new("/workspace"),
        &root_entries,
        500.0,
        96,
        Some(&state),
    );
    let band = first_band(&layout);

    assert_eq!(band.height, band.natural_height * 0.5);
    assert!(!band.interactive);
}

#[test]
fn keyboard_navigation_crosses_from_root_row_into_expansion_panel() {
    let root_entries = vec![entry("/workspace/root", FileKind::Directory)];
    let state = expansion(vec![
        entry("/workspace/root/a", FileKind::File),
        entry("/workspace/root/b", FileKind::File),
    ]);
    let layout = IconGridLayout::new(
        Path::new("/workspace"),
        &root_entries,
        500.0,
        96,
        Some(&state),
    );

    let down = layout
        .keyboard_target(Some(Path::new("/workspace/root")), IconGridDirection::Down)
        .unwrap();
    assert_eq!(down.entry.path, Path::new("/workspace/root/a"));
    assert_eq!(down.directory, Path::new("/workspace/root"));
    let left = layout
        .keyboard_target(Some(&down.entry.path), IconGridDirection::Left)
        .unwrap();
    assert_eq!(left.entry.path, Path::new("/workspace/root"));
}

#[test]
fn resize_reflows_anchor_to_new_row_without_changing_state_index() {
    let root_entries = vec![
        entry("/workspace/a", FileKind::File),
        entry("/workspace/b", FileKind::File),
        entry("/workspace/root", FileKind::Directory),
    ];
    let state = IconGridExpansionState::new(
        IconGridExpansionContext {
            pane_id: BrowserPaneId(1),
            current_dir: PathBuf::from("/workspace"),
            session_id: IconGridExpansionSessionId::new(1),
        },
        anchor("/workspace", "/workspace/root", 2),
        loaded(files("/workspace/root", 1)),
    );
    let wide = IconGridLayout::new(
        Path::new("/workspace"),
        &root_entries,
        500.0,
        96,
        Some(&state),
    );
    let narrow = IconGridLayout::new(
        Path::new("/workspace"),
        &root_entries,
        200.0,
        96,
        Some(&state),
    );

    assert_eq!(
        first_band(&wide).top,
        ICON_GRID_CONTENT_PADDING + row_height(96)
    );
    assert_eq!(
        first_band(&narrow).top,
        ICON_GRID_CONTENT_PADDING + row_height(96) * 3.0
    );
}

#[test]
fn large_flat_directory_stays_one_compressed_segment() {
    let entries = files("/workspace", 100_000);
    let layout = IconGridLayout::new(Path::new("/workspace"), &entries, 500.0, 96, None);

    assert_eq!(layout.root().flow.len(), 1);
}
