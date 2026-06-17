use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use file_core::{DirectoryEntry, EntryMetadata, FileKind};

use crate::app::FileBrowser;
use crate::config;
use crate::model::{
    BrowserPane, BrowserPaneId, BrowserPaneLayout, BrowserTab, BrowserViewMode, ExpandedDirectory,
    ExpandedDirectoryStatus, SplitAxis, StartupEnvironment,
};
use crate::thumbnail_cache::ColumnViewport;

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

fn loaded_directory(entries: Vec<DirectoryEntry>) -> ExpandedDirectory {
    ExpandedDirectory {
        entries,
        status: ExpandedDirectoryStatus::Loaded,
        is_expanded: true,
        is_collapsing: false,
        animation_progress: 1.0,
        load_generation: 0,
        load_cancel: None,
    }
}

fn pane_from_tab_for_test(pane_id: BrowserPaneId, tab: BrowserTab) -> BrowserPane {
    BrowserPane {
        id: pane_id,
        current_dir: tab.directory.clone(),
        is_trash_view: tab.is_trash_view,
        entries: tab.entries.clone(),
        directory_loading_placeholder_entries: Vec::new(),
        trash_entries: tab.trash_entries.clone(),
        selected: tab.selected.clone(),
        selected_paths: tab.selected_paths.clone(),
        selection_anchor: tab.selection_anchor.clone(),
        deepest_open_column_directory: tab.deepest_open_column_directory.clone(),
        expanded_directories: tab.expanded_directories.clone(),
        view_mode: tab.view_mode,
        column_viewports: HashMap::<PathBuf, ColumnViewport>::new(),
        tabs: vec![tab.clone()],
        active_tab_id: tab.id,
        path_input: tab.directory.to_string_lossy().into_owned(),
        path_suggestions: Vec::new(),
        path_suggestion_selection: None,
        path_suggestion_generation: 0,
        directory_load_generation: 0,
        directory_load_cancel: None,
        back_stack: tab.back_stack.clone(),
        forward_stack: tab.forward_stack.clone(),
        is_loading: false,
    }
}

#[test]
fn new_browser_uses_configured_default_view_mode() {
    let mut config = config::default_user_config();
    config.browser_view_mode = BrowserViewMode::List;

    let (browser, _) = FileBrowser::new(config);

    assert_eq!(browser.view_mode, BrowserViewMode::List);
    assert!(browser
        .tabs
        .iter()
        .all(|tab| tab.view_mode == BrowserViewMode::List));
    assert!(browser
        .pane_by_id(BrowserPaneId::PRIMARY)
        .is_some_and(|pane| pane.view_mode == BrowserViewMode::List));
}

#[test]
fn loaded_user_config_updates_startup_view_mode() {
    let (mut browser, _) = FileBrowser::new(config::ui_thread_startup_config());
    let mut user_config = config::default_user_config();
    user_config.browser_view_mode = BrowserViewMode::List;

    drop(browser.accept_startup_environment(StartupEnvironment {
        home: PathBuf::from("/home/user"),
        user_config,
        state_database_path: PathBuf::from("/tmp/state.sqlite"),
    }));

    assert_eq!(browser.view_mode, BrowserViewMode::List);
    assert!(browser
        .tabs
        .iter()
        .find(|tab| tab.id == browser.active_tab_id)
        .is_some_and(|tab| tab.view_mode == BrowserViewMode::List));
    assert!(browser
        .pane_by_id(BrowserPaneId::PRIMARY)
        .is_some_and(|pane| pane.view_mode == BrowserViewMode::List));
}

#[test]
fn selecting_view_mode_updates_persisted_default_view_mode() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());

    drop(browser.select_browser_view_mode(BrowserPaneId::PRIMARY, BrowserViewMode::List));

    assert_eq!(browser.view_mode, BrowserViewMode::List);
    assert_eq!(browser.user_config.browser_view_mode, BrowserViewMode::List);
}

#[test]
fn switching_tabs_restores_view_mode_and_expanded_directories() {
    let root = PathBuf::from("/workspace");
    let active_directory = root.join("active");
    let active_child = active_directory.join("child.txt");
    let other_directory = root.join("other");
    let other_child = other_directory.join("child.txt");
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    browser.current_dir = root.clone();
    browser.entries = vec![test_entry(active_directory.clone(), FileKind::Directory)];
    browser.view_mode = BrowserViewMode::List;
    browser.expanded_directories.insert(
        active_directory.clone(),
        loaded_directory(vec![test_entry(active_child.clone(), FileKind::File)]),
    );
    browser.tabs = vec![
        BrowserTab::directory(0, root.clone()),
        BrowserTab::directory(1, other_directory.clone()),
    ];
    browser.active_tab_id = 0;
    browser.tabs[1].entries = vec![test_entry(other_directory.clone(), FileKind::Directory)];
    browser.tabs[1].view_mode = BrowserViewMode::Columns;
    browser.tabs[1].expanded_directories.insert(
        other_directory.clone(),
        loaded_directory(vec![test_entry(other_child.clone(), FileKind::File)]),
    );

    drop(browser.select_tab(1));

    assert_eq!(browser.view_mode, BrowserViewMode::Columns);
    assert!(browser.expanded_directories.contains_key(&other_directory));

    drop(browser.select_tab(0));

    assert_eq!(browser.view_mode, BrowserViewMode::List);
    assert!(browser.expanded_directories.contains_key(&active_directory));
    assert!(browser
        .tabs
        .iter()
        .find(|tab| tab.id == 1)
        .is_some_and(|tab| tab.view_mode == BrowserViewMode::Columns
            && tab.expanded_directories.contains_key(&other_directory)));
}

#[test]
fn activating_split_panes_preserves_each_pane_view_mode() {
    let left_dir = PathBuf::from("/workspace/left");
    let right_dir = PathBuf::from("/workspace/right");
    let left_tab = BrowserTab::directory(0, left_dir.clone());
    let mut right_tab = BrowserTab::directory(1, right_dir.clone());
    right_tab.view_mode = BrowserViewMode::Columns;
    right_tab.selected_paths = HashSet::new();

    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    browser.current_dir = left_dir.clone();
    browser.tabs = vec![left_tab.clone()];
    browser.active_tab_id = left_tab.id;
    browser.view_mode = BrowserViewMode::List;
    browser.pane_layout = BrowserPaneLayout::Split {
        axis: SplitAxis::Horizontal,
        first: BrowserPaneId::PRIMARY,
        second: BrowserPaneId(1),
        active: BrowserPaneId::PRIMARY,
    };
    browser.panes = vec![
        pane_from_tab_for_test(BrowserPaneId::PRIMARY, left_tab),
        pane_from_tab_for_test(BrowserPaneId(1), right_tab),
    ];

    browser.activate_pane(BrowserPaneId(1));

    assert_eq!(browser.current_dir, right_dir);
    assert_eq!(browser.view_mode, BrowserViewMode::Columns);

    browser.view_mode = BrowserViewMode::Columns;
    browser.activate_pane(BrowserPaneId::PRIMARY);

    assert_eq!(browser.current_dir, left_dir);
    assert_eq!(browser.view_mode, BrowserViewMode::List);
    assert!(browser
        .pane_by_id(BrowserPaneId(1))
        .is_some_and(|pane| pane.view_mode == BrowserViewMode::Columns));
}
