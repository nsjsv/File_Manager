use std::collections::HashSet;
use std::path::PathBuf;

use file_core::{DirectoryEntry, EntryMetadata, FileKind};
use iced::{Point, Rectangle};

use crate::{
    app::FileBrowser,
    config,
    model::{
        BrowserPaneId, ColumnEntryBounds, ContextMenuState, FileDragTarget, SelectionMarquee,
        SelectionMarqueePhase, SelectionMarqueeSource,
    },
};

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

fn active_selection_marquee(
    current: Point,
    base_selection: HashSet<PathBuf>,
    preserve_existing: bool,
) -> SelectionMarquee {
    SelectionMarquee {
        start: Point::new(0.0, 0.0),
        current,
        source: SelectionMarqueeSource::PaneBlank,
        phase: SelectionMarqueePhase::Selecting,
        base_selection,
        preserve_existing,
    }
}

fn entry_bounds(path: PathBuf, x: f32, y: f32, width: f32, height: f32) -> ColumnEntryBounds {
    ColumnEntryBounds {
        pane_id: BrowserPaneId::PRIMARY,
        path,
        bounds: Rectangle {
            x,
            y,
            width,
            height,
        },
    }
}

#[test]
fn rectangles_intersect_only_when_areas_overlap() {
    let first = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 20.0,
    };

    assert!(super::rectangles_intersect(
        first,
        Rectangle {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
        }
    ));
    assert!(!super::rectangles_intersect(
        first,
        Rectangle {
            x: 20.0,
            y: 0.0,
            width: 20.0,
            height: 20.0,
        }
    ));
}

#[test]
fn drag_target_falls_back_from_source_parent_to_pane_directory() {
    let source = PathBuf::from("/right/file.txt");
    let source_parent = PathBuf::from("/right");
    let fallback = PathBuf::from("/left");

    let target = super::resolve_file_drag_target(
        &[source],
        None,
        Some(FileDragTarget::Directory(source_parent)),
        Some(fallback.clone()),
    );

    assert!(matches!(target, Some(FileDragTarget::Directory(path)) if path == fallback));
}

#[test]
fn drag_target_keeps_hovered_directory_in_target_pane() {
    let source = PathBuf::from("/right/file.txt");
    let hovered = PathBuf::from("/left/folder");
    let fallback = PathBuf::from("/left");

    let target = super::resolve_file_drag_target(
        &[source],
        None,
        Some(FileDragTarget::Directory(hovered.clone())),
        Some(fallback),
    );

    assert!(matches!(target, Some(FileDragTarget::Directory(path)) if path == hovered));
}

#[test]
fn drag_target_prefers_release_column_over_stale_hover_target() {
    let source = PathBuf::from("/right/file.txt");
    let stale_hover_target = PathBuf::from("/right");
    let release_column = PathBuf::from("/left/actual-column");
    let fallback = PathBuf::from("/left");

    let target = super::resolve_file_drag_target(
        &[source],
        Some(release_column.clone()),
        Some(FileDragTarget::Directory(stale_hover_target)),
        Some(fallback),
    );

    assert!(matches!(target, Some(FileDragTarget::Directory(path)) if path == release_column));
}

#[test]
fn drag_target_uses_pane_directory_when_hover_target_missing() {
    let source = PathBuf::from("/right/file.txt");
    let fallback = PathBuf::from("/left");

    let target = super::resolve_file_drag_target(&[source], None, None, Some(fallback.clone()));

    assert!(matches!(target, Some(FileDragTarget::Directory(path)) if path == fallback));
}

#[test]
fn marquee_selection_uses_intersecting_bounds() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let current_dir = PathBuf::from("/workspace");
    let inside = current_dir.join("inside.txt");
    let outside = current_dir.join("outside.txt");
    browser.current_dir = current_dir;
    browser.entries = vec![
        test_entry(inside.clone(), FileKind::File),
        test_entry(outside.clone(), FileKind::File),
    ];
    browser.selection_marquee = Some(active_selection_marquee(
        Point::new(50.0, 50.0),
        HashSet::new(),
        false,
    ));

    let command = browser.update_selection_from_column_entry_bounds(vec![
        entry_bounds(inside.clone(), 10.0, 10.0, 20.0, 20.0),
        entry_bounds(outside.clone(), 70.0, 70.0, 20.0, 20.0),
    ]);
    drop(command);

    assert!(browser.is_path_selected(&inside));
    assert!(!browser.is_path_selected(&outside));
    assert_eq!(browser.selected.as_ref(), Some(&inside));
}

#[test]
fn marquee_selection_preserves_existing_selection_when_requested() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let current_dir = PathBuf::from("/workspace");
    let preserved = current_dir.join("preserved.txt");
    let added = current_dir.join("added.txt");
    browser.current_dir = current_dir;
    browser.entries = vec![
        test_entry(preserved.clone(), FileKind::File),
        test_entry(added.clone(), FileKind::File),
    ];
    browser.selection_marquee = Some(active_selection_marquee(
        Point::new(50.0, 50.0),
        HashSet::from([preserved.clone()]),
        true,
    ));

    let command = browser.update_selection_from_column_entry_bounds(vec![entry_bounds(
        added.clone(),
        10.0,
        10.0,
        20.0,
        20.0,
    )]);
    drop(command);

    assert!(browser.is_path_selected(&preserved));
    assert!(browser.is_path_selected(&added));
}

#[test]
fn clicking_current_column_blank_clears_existing_selection() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let current_dir = PathBuf::from("/workspace");
    let first = current_dir.join("first.txt");
    let second = current_dir.join("second.txt");
    let child_directory = current_dir.join("project");
    browser.current_dir = current_dir.clone();
    browser.entries = vec![
        test_entry(first.clone(), FileKind::File),
        test_entry(second.clone(), FileKind::File),
        test_entry(child_directory.clone(), FileKind::Directory),
    ];
    browser.selected = Some(second.clone());
    browser.selected_paths = HashSet::from([first, second]);
    browser.selection_anchor = Some(child_directory.clone());

    let command = browser.handle_column_blank_clicked(current_dir.clone());
    drop(command);

    assert!(browser.selected.is_none());
    assert!(browser.selected_paths.is_empty());
    assert!(browser.selection_anchor.is_none());
    assert_eq!(
        browser.path_input,
        crate::app::paths::path_text(&current_dir)
    );
}

#[test]
fn clicking_child_column_blank_preserves_open_column_context() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let current_dir = PathBuf::from("/workspace");
    let first = current_dir.join("first.txt");
    let second = current_dir.join("second.txt");
    let child_directory = current_dir.join("project");
    browser.current_dir = current_dir;
    browser.entries = vec![
        test_entry(first.clone(), FileKind::File),
        test_entry(second.clone(), FileKind::File),
        test_entry(child_directory.clone(), FileKind::Directory),
    ];
    browser.selected = Some(second);
    browser.selected_paths = HashSet::from([first]);

    let command = browser.handle_column_blank_clicked(child_directory.clone());
    drop(command);

    assert_eq!(
        browser.deepest_open_column_directory.as_ref(),
        Some(&child_directory)
    );
    assert_eq!(browser.selected.as_ref(), Some(&child_directory));
    assert_eq!(
        browser.selected_paths,
        HashSet::from([child_directory.clone()])
    );
    assert_eq!(browser.selection_anchor.as_ref(), Some(&child_directory));
    assert_eq!(
        browser.path_input,
        crate::app::paths::path_text(&child_directory)
    );
}

#[test]
fn pressing_current_column_blank_clears_selection_before_release() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let current_dir = PathBuf::from("/workspace");
    let first = current_dir.join("first.txt");
    let second = current_dir.join("second.txt");
    let child_directory = current_dir.join("project");
    browser.current_dir = current_dir.clone();
    browser.entries = vec![
        test_entry(first.clone(), FileKind::File),
        test_entry(second.clone(), FileKind::File),
        test_entry(child_directory.clone(), FileKind::Directory),
    ];
    browser.selected = Some(second.clone());
    browser.selected_paths = HashSet::from([first, second]);
    browser.selection_anchor = Some(child_directory.clone());

    let command = browser.start_column_blank_selection_marquee(current_dir.clone());
    drop(command);

    assert!(browser.selected.is_none());
    assert!(browser.selected_paths.is_empty());
    assert!(browser.selection_anchor.is_none());
    assert!(browser.selection_marquee.is_some());
    assert_eq!(
        browser.path_input,
        crate::app::paths::path_text(&current_dir)
    );
}

#[test]
fn pressing_child_column_blank_preserves_open_column_before_release() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let current_dir = PathBuf::from("/workspace");
    let first = current_dir.join("first.txt");
    let second = current_dir.join("second.txt");
    let child_directory = current_dir.join("project");
    browser.current_dir = current_dir;
    browser.entries = vec![
        test_entry(first.clone(), FileKind::File),
        test_entry(second.clone(), FileKind::File),
        test_entry(child_directory.clone(), FileKind::Directory),
    ];
    browser.selected = Some(second);
    browser.selected_paths = HashSet::from([first]);

    let command = browser.start_column_blank_selection_marquee(child_directory.clone());
    drop(command);

    assert_eq!(
        browser.deepest_open_column_directory.as_ref(),
        Some(&child_directory)
    );
    assert_eq!(browser.selected.as_ref(), Some(&child_directory));
    assert_eq!(
        browser.selected_paths,
        HashSet::from([child_directory.clone()])
    );
    assert_eq!(browser.selection_anchor.as_ref(), Some(&child_directory));
    assert!(browser.selection_marquee.is_some());
    assert_eq!(
        browser.path_input,
        crate::app::paths::path_text(&child_directory)
    );
}

#[test]
fn dragging_child_column_blank_preserves_open_column_context() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let current_dir = PathBuf::from("/workspace");
    let first = current_dir.join("first.txt");
    let second = current_dir.join("second.txt");
    let child_directory = current_dir.join("project");
    browser.current_dir = current_dir;
    browser.entries = vec![
        test_entry(first.clone(), FileKind::File),
        test_entry(second.clone(), FileKind::File),
        test_entry(child_directory.clone(), FileKind::Directory),
    ];
    browser.selected = Some(second);
    browser.selected_paths = HashSet::from([first]);

    let press_command = browser.start_column_blank_selection_marquee(child_directory.clone());
    drop(press_command);
    browser
        .selection_marquee
        .as_mut()
        .expect("selection marquee starts")
        .phase = SelectionMarqueePhase::Selecting;

    let drag_command = browser.update_selection_from_column_entry_bounds(Vec::new());
    drop(drag_command);

    assert_eq!(
        browser.deepest_open_column_directory.as_ref(),
        Some(&child_directory)
    );
    assert_eq!(
        crate::three_column_view::column_directories(&browser),
        vec![browser.current_dir.clone(), child_directory.clone()]
    );
    assert!(browser.selected.is_none());
    assert!(browser.selected_paths.is_empty());
    assert_eq!(
        browser.path_input,
        crate::app::paths::path_text(&browser.current_dir)
    );
}

#[test]
fn releasing_selected_item_without_drag_collapses_multi_selection() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let current_dir = PathBuf::from("/workspace");
    let first = current_dir.join("first.txt");
    let second = current_dir.join("second.txt");
    browser.current_dir = current_dir;
    browser.entries = vec![
        test_entry(first.clone(), FileKind::File),
        test_entry(second.clone(), FileKind::File),
    ];
    browser.selected = Some(first.clone());
    browser.selected_paths = HashSet::from([first.clone(), second.clone()]);

    let press_command = browser.handle_column_entry_clicked(second.clone());
    drop(press_command);
    assert_eq!(
        browser.selected_paths,
        HashSet::from([first, second.clone()])
    );

    let release_command = browser.finish_drag_selection(None);
    drop(release_command);

    assert_eq!(browser.selected.as_ref(), Some(&second));
    assert_eq!(browser.selected_paths, HashSet::from([second.clone()]));
    assert_eq!(browser.selection_anchor.as_ref(), Some(&second));
}

#[test]
fn right_clicking_directory_selects_menu_target_without_focusing_it() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let current_dir = PathBuf::from("/workspace");
    let directory = current_dir.join("project");
    browser.current_dir = current_dir.clone();
    browser.entries = vec![test_entry(directory.clone(), FileKind::Directory)];
    browser.selected = None;
    browser.selected_paths.clear();
    browser.expanded_directories.clear();

    let command = browser.handle_entry_right_clicked(directory.clone());
    drop(command);

    assert_eq!(browser.current_dir, current_dir);
    assert!(browser.selected.is_none());
    assert!(browser.is_path_selected(&directory));
    assert!(!browser.expanded_directories.contains_key(&directory));

    let ContextMenuState::FileArea(context_menu) =
        browser.context_menu.as_ref().expect("context menu opens")
    else {
        panic!("file context menu opens");
    };
    assert_eq!(context_menu.target.as_ref(), Some(&directory));
    assert!(context_menu.target_is_directory);
}
