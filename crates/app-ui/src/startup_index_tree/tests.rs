use super::*;
use file_core::{EntryMetadata, FileKind};
use std::path::{Path, PathBuf};

#[test]
fn setup_requires_target_mode_and_capability_before_accept() {
    let mut setup = StartupIndexSetupState::from_choices(
        vec![StartupIndexRootSeed {
            label: "Documents".to_owned(),
            path: PathBuf::from("/home/user/Documents"),
            selection: StartupIndexEntrySelection::Selected,
        }],
        None,
    )
    .expect("setup");

    assert!(!setup.can_accept());
    setup.select_capability(StartupIndexCapability::Filenames);
    assert!(!setup.can_accept());
    setup.select_target_mode(StartupIndexTargetMode::Common);
    assert!(setup.can_accept());

    let mut custom_setup = StartupIndexSetupState::from_choices(
        Vec::new(),
        Some(StartupIndexRootSeed {
            label: "Home".to_owned(),
            path: PathBuf::from("/home/user"),
            selection: StartupIndexEntrySelection::Skipped,
        }),
    )
    .expect("custom setup");
    custom_setup.select_capability(StartupIndexCapability::Filenames);
    custom_setup.select_target_mode(StartupIndexTargetMode::Custom);
    assert!(!custom_setup.can_accept());
    custom_setup.toggle_entry_selection(0);
    assert!(custom_setup.can_accept());
}

#[test]
fn custom_selected_requests_use_specific_roots_and_reduce_nested_paths() {
    let home = PathBuf::from("/home/user");
    let docs = home.join("Documents");
    let nested = docs.join("note.txt");
    let root_file = home.join("todo.txt");
    let mut setup = StartupIndexSetupState::from_choices(
        Vec::new(),
        Some(StartupIndexRootSeed {
            label: "Home".to_owned(),
            path: home.clone(),
            selection: StartupIndexEntrySelection::Skipped,
        }),
    )
    .expect("custom setup");
    setup.select_target_mode(StartupIndexTargetMode::Custom);
    setup.accept_directory_children(
        &home,
        vec![
            test_entry(docs.clone(), FileKind::Directory),
            test_entry(root_file.clone(), FileKind::File),
        ],
    );
    setup.toggle_directory(1);
    setup.accept_directory_children(&docs, vec![test_entry(nested, FileKind::File)]);
    setup.toggle_entry_selection(1);
    setup.toggle_entry_selection(3);

    let requests = setup.selected_index_requests();

    assert_eq!(
        requests,
        vec![
            StartupIndexBuildRequest {
                root: docs.clone(),
                selected_paths: vec![docs],
            },
            StartupIndexBuildRequest {
                root: home,
                selected_paths: vec![root_file],
            },
        ]
    );
}

#[test]
fn shift_range_selects_visible_entries_between_anchor_and_target() {
    let home = PathBuf::from("/home/user");
    let docs = home.join("Documents");
    let downloads = home.join("Downloads");
    let pictures = home.join("Pictures");
    let mut setup = custom_tree_with_home_children(
        &home,
        vec![
            test_entry(docs.clone(), FileKind::Directory),
            test_entry(downloads.clone(), FileKind::Directory),
            test_entry(pictures.clone(), FileKind::Directory),
        ],
    );

    setup.toggle_entry_selection(1);
    setup.select_entry_range(3, &[0, 1, 2, 3]);

    assert_eq!(selected_paths(&setup), vec![docs, downloads, pictures]);
}

#[test]
fn shift_range_uses_anchor_selection_for_reverse_deselection() {
    let home = PathBuf::from("/home/user");
    let docs = home.join("Documents");
    let downloads = home.join("Downloads");
    let pictures = home.join("Pictures");
    let mut setup = custom_tree_with_home_children(
        &home,
        vec![
            test_entry(docs, FileKind::Directory),
            test_entry(downloads, FileKind::Directory),
            test_entry(pictures, FileKind::Directory),
        ],
    );
    setup.start_entry_selection_drag(1);
    setup.enter_entry_during_selection_drag(2, &[0, 1, 2, 3]);
    setup.enter_entry_during_selection_drag(3, &[0, 1, 2, 3]);
    setup.finish_entry_selection_drag();

    setup.toggle_entry_selection(1);
    setup.select_entry_range(3, &[0, 1, 2, 3]);

    assert!(selected_paths(&setup).is_empty());
}

#[test]
fn dragging_over_entries_selects_each_entered_entry() {
    let home = PathBuf::from("/home/user");
    let docs = home.join("Documents");
    let downloads = home.join("Downloads");
    let mut setup = custom_tree_with_home_children(
        &home,
        vec![
            test_entry(docs.clone(), FileKind::Directory),
            test_entry(downloads.clone(), FileKind::Directory),
        ],
    );

    setup.start_entry_selection_drag(1);
    setup.enter_entry_during_selection_drag(2, &[0, 1, 2]);
    setup.finish_entry_selection_drag();
    setup.enter_entry_during_selection_drag(0, &[0, 1, 2]);

    assert_eq!(selected_paths(&setup), vec![docs, downloads]);
    assert!(!setup.entries[0].selection.is_selected());
}

#[test]
fn dragging_to_non_adjacent_entry_selects_visible_entries_between() {
    let home = PathBuf::from("/home/user");
    let docs = home.join("Documents");
    let downloads = home.join("Downloads");
    let pictures = home.join("Pictures");
    let mut setup = custom_tree_with_home_children(
        &home,
        vec![
            test_entry(docs.clone(), FileKind::Directory),
            test_entry(downloads.clone(), FileKind::Directory),
            test_entry(pictures.clone(), FileKind::Directory),
        ],
    );

    setup.start_entry_selection_drag(1);
    setup.enter_entry_during_selection_drag(3, &[0, 1, 2, 3]);
    setup.finish_entry_selection_drag();

    assert_eq!(selected_paths(&setup), vec![docs, downloads, pictures]);
}

#[test]
fn dragging_from_selected_entry_deselects_each_entered_entry() {
    let home = PathBuf::from("/home/user");
    let docs = home.join("Documents");
    let downloads = home.join("Downloads");
    let mut setup = custom_tree_with_home_children(
        &home,
        vec![
            test_entry(docs, FileKind::Directory),
            test_entry(downloads, FileKind::Directory),
        ],
    );
    setup.start_entry_selection_drag(1);
    setup.enter_entry_during_selection_drag(2, &[0, 1, 2]);
    setup.finish_entry_selection_drag();

    setup.start_entry_selection_drag(1);
    setup.enter_entry_during_selection_drag(2, &[0, 1, 2]);
    setup.finish_entry_selection_drag();

    assert!(selected_paths(&setup).is_empty());
}

fn custom_tree_with_home_children(
    home: &Path,
    children: Vec<DirectoryEntry>,
) -> StartupIndexSetupState {
    let mut setup = StartupIndexSetupState::from_choices(
        Vec::new(),
        Some(StartupIndexRootSeed {
            label: "Home".to_owned(),
            path: home.to_path_buf(),
            selection: StartupIndexEntrySelection::Skipped,
        }),
    )
    .expect("custom setup");
    setup.select_target_mode(StartupIndexTargetMode::Custom);
    setup.accept_directory_children(home, children);
    setup
}

fn selected_paths(setup: &StartupIndexSetupState) -> Vec<PathBuf> {
    setup
        .entries
        .iter()
        .filter(|entry| entry.selection.is_selected())
        .map(|entry| entry.path.clone())
        .collect()
}

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
