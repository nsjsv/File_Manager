use std::collections::HashSet;
use std::path::PathBuf;

use file_core::{DirectoryEntry, DirectoryScan, EntryMetadata, FileKind};
use iced::{keyboard, Point};

use crate::app::FileBrowser;
use crate::config;
use crate::model::{BrowserPaneId, ExpandedDirectory, ExpandedDirectoryStatus, FileDragPhase};
use crate::shortcuts::FileSelectionDirection;

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

fn browser_with_entries(paths: &[PathBuf]) -> FileBrowser {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    browser.current_dir = PathBuf::from("/workspace");
    browser.entries = paths
        .iter()
        .cloned()
        .map(|path| test_entry(path, FileKind::File))
        .collect();
    browser
}

fn loaded_directory(entries: Vec<DirectoryEntry>) -> ExpandedDirectory {
    ExpandedDirectory {
        entries,
        status: ExpandedDirectoryStatus::Loaded,
        is_expanded: true,
        animation_progress: 1.0,
    }
}

#[test]
fn shift_range_click_does_not_seed_activation_double_click() {
    let first = PathBuf::from("/workspace/first.txt");
    let second = PathBuf::from("/workspace/second.txt");
    let mut browser = browser_with_entries(&[first.clone(), second.clone()]);

    drop(browser.handle_column_entry_clicked(first.clone()));
    browser.keyboard_modifiers = keyboard::Modifiers::SHIFT;
    drop(browser.handle_column_entry_clicked(second.clone()));

    assert!(browser.last_activation_click.is_none());
    assert_eq!(
        browser.selected_paths,
        HashSet::from([first.clone(), second.clone()])
    );

    browser.keyboard_modifiers = keyboard::Modifiers::default();
    drop(browser.handle_column_entry_clicked(second.clone()));

    let file_drag = browser
        .file_drag
        .as_ref()
        .expect("plain click after range selection starts file drag");
    assert_eq!(file_drag.sources, vec![first, second]);
    assert!(matches!(
        file_drag.phase,
        FileDragPhase::WaitingForMovement { .. }
    ));

    browser.update_file_drag(Point::new(10.0, 0.0));
    assert!(browser
        .file_drag
        .as_ref()
        .is_some_and(|file_drag| file_drag.is_dragging()));
}

#[test]
fn control_selection_click_does_not_seed_activation_double_click() {
    let first = PathBuf::from("/workspace/first.txt");
    let second = PathBuf::from("/workspace/second.txt");
    let mut browser = browser_with_entries(&[first.clone(), second.clone()]);

    drop(browser.handle_column_entry_clicked(first.clone()));
    browser.keyboard_modifiers = keyboard::Modifiers::CTRL;
    drop(browser.handle_column_entry_clicked(second.clone()));

    assert!(browser.last_activation_click.is_none());
    assert_eq!(
        browser.selected_paths,
        HashSet::from([first, second.clone()])
    );

    browser.keyboard_modifiers = keyboard::Modifiers::default();
    drop(browser.handle_column_entry_clicked(second));

    assert!(browser.file_drag.is_some());
}

#[test]
fn plain_double_click_still_activates_directory() {
    let directory = PathBuf::from("/workspace/project");
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    browser.current_dir = PathBuf::from("/workspace");
    browser.entries = vec![test_entry(directory.clone(), FileKind::Directory)];

    drop(browser.handle_column_entry_clicked(directory.clone()));
    drop(browser.handle_column_entry_clicked(directory.clone()));

    assert_eq!(browser.current_dir, directory);
    assert!(matches!(browser.file_drag, None));
    assert!(browser.last_activation_click.is_none());
    assert_eq!(browser.back_stack, vec![PathBuf::from("/workspace")]);
    assert!(matches!(browser.is_trash_view, false));
}

#[test]
fn right_arrow_enters_loaded_child_column_and_remembers_return_target() {
    let parent = PathBuf::from("/workspace/project");
    let first_child = PathBuf::from("/workspace/project/a.txt");
    let return_child = PathBuf::from("/workspace/project/notes.txt");
    let mut browser = browser_with_entries(&[parent.clone()]);
    browser.entries = vec![test_entry(parent.clone(), FileKind::Directory)];
    browser.expanded_directories.insert(
        parent.clone(),
        loaded_directory(vec![
            test_entry(first_child.clone(), FileKind::File),
            test_entry(return_child.clone(), FileKind::File),
        ]),
    );
    browser.deepest_open_column_directory = Some(parent.clone());

    browser.select_path(return_child.clone());
    drop(browser.move_file_selection(FileSelectionDirection::Left));

    assert_eq!(browser.selected, Some(parent.clone()));
    assert_eq!(browser.deepest_open_column_directory, Some(parent.clone()));

    drop(browser.move_file_selection(FileSelectionDirection::Right));

    assert_eq!(browser.selected, Some(return_child));
    assert_ne!(browser.selected, Some(first_child));
    assert_eq!(browser.deepest_open_column_directory, Some(parent));
}

#[test]
fn vertical_arrows_open_directory_contents_without_entering_child_column() {
    let first_parent = PathBuf::from("/workspace/a");
    let second_parent = PathBuf::from("/workspace/b");
    let second_child = PathBuf::from("/workspace/b/child.txt");
    let mut browser = browser_with_entries(&[first_parent.clone(), second_parent.clone()]);
    browser.entries = vec![
        test_entry(first_parent.clone(), FileKind::Directory),
        test_entry(second_parent.clone(), FileKind::Directory),
    ];
    browser.expanded_directories.insert(
        second_parent.clone(),
        loaded_directory(vec![test_entry(second_child.clone(), FileKind::File)]),
    );
    browser.select_path(first_parent);

    drop(browser.move_file_selection(FileSelectionDirection::Down));

    assert_eq!(browser.selected, Some(second_parent.clone()));
    assert_eq!(
        browser.selected_paths,
        HashSet::from([second_parent.clone()])
    );
    assert_eq!(browser.deepest_open_column_directory, Some(second_parent));
    assert_ne!(browser.selected, Some(second_child));
}

#[test]
fn right_arrow_focuses_first_child_after_directory_loads() {
    let parent = PathBuf::from("/workspace/project");
    let child = PathBuf::from("/workspace/project/first.txt");
    let mut browser = browser_with_entries(&[parent.clone()]);
    browser.entries = vec![test_entry(parent.clone(), FileKind::Directory)];
    browser.select_path(parent.clone());

    drop(browser.move_file_selection(FileSelectionDirection::Right));

    assert_eq!(browser.selected, Some(parent.clone()));
    assert!(browser.pending_keyboard_column_focus.is_some());

    drop(browser.accept_expanded_directory(
        BrowserPaneId::PRIMARY,
        parent.clone(),
        Ok(DirectoryScan {
            path: parent.clone(),
            entries: vec![test_entry(child.clone(), FileKind::File)],
            skipped: Vec::new(),
        }),
    ));

    assert_eq!(browser.selected, Some(child));
    assert!(browser.pending_keyboard_column_focus.is_none());
    assert_eq!(browser.deepest_open_column_directory, Some(parent));
}
