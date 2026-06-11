use std::collections::HashSet;
use std::path::PathBuf;

use file_core::{DirectoryEntry, EntryMetadata, FileKind};
use iced::{keyboard, Point};

use crate::app::FileBrowser;
use crate::config;
use crate::model::FileDragPhase;

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
