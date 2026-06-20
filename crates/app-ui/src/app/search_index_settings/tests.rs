use std::path::{Path, PathBuf};

use super::*;
use crate::app::FileBrowser;
use crate::model::{
    SearchIndexPathRuleEditMode, SearchIndexPathRuleKind, SearchIndexPathRuleSelection,
};

#[test]
fn adding_parent_root_keeps_explicit_nested_roots() {
    let roots = search_index_roots_with_added_root(
        &[PathBuf::from("/workspace/project/src")],
        PathBuf::from("/workspace/project"),
    );

    assert_eq!(
        roots,
        vec![
            PathBuf::from("/workspace/project/src"),
            PathBuf::from("/workspace/project"),
        ]
    );
}

#[test]
fn adding_nested_root_is_kept_when_parent_is_already_indexed() {
    let existing = vec![PathBuf::from("/workspace/project")];
    let roots =
        search_index_roots_with_added_root(&existing, PathBuf::from("/workspace/project/src"));

    assert_eq!(
        roots,
        vec![
            PathBuf::from("/workspace/project"),
            PathBuf::from("/workspace/project/src"),
        ]
    );
}

#[test]
fn default_exclude_patterns_are_visible_as_rule_inputs() {
    let (mut browser, _) = FileBrowser::new(crate::config::default_user_config());

    browser.sync_search_index_exclude_inputs_from_config();

    assert_eq!(
        browser.search_index.exclude_pattern_inputs,
        file_index::default_search_index_exclude_patterns()
            .iter()
            .map(|pattern| (*pattern).to_owned())
            .collect::<Vec<_>>()
    );
}

#[test]
fn empty_exclude_pattern_config_stays_visible_as_empty_rules() {
    let mut config = crate::config::default_user_config();
    config.search_index_exclude_patterns.clear();
    let (mut browser, _) = FileBrowser::new(config);

    browser.sync_search_index_exclude_inputs_from_config();

    assert!(browser.search_index.exclude_pattern_inputs.is_empty());
}

#[test]
fn global_exclude_pattern_displays_without_home_root_prefix() {
    let label = search_index_exclude_pattern_display_path(
        &[PathBuf::from("/home/user")],
        "node_modules/",
        Path::new("/home/user"),
    );

    assert_eq!(label.as_deref(), Some("node_modules/"));
}

#[test]
fn loaded_empty_profile_keeps_configured_default_exclude_patterns() {
    let mut browser = browser_with_search_index_home();
    let profile = file_index::IndexProfile::new("default", vec![PathBuf::from("/home/user")]);

    let _task = browser.accept_search_index_profile(Ok(Some(profile)));

    let expected = file_index::default_search_index_exclude_patterns()
        .iter()
        .map(|pattern| (*pattern).to_owned())
        .collect::<Vec<_>>();
    assert_eq!(browser.search_index.exclude_pattern_inputs, expected);
    assert_eq!(browser.user_config.search_index_exclude_patterns, expected);
}

#[test]
fn loaded_empty_profile_preserves_explicit_empty_exclude_config() {
    let mut config = crate::config::default_user_config();
    config.search_index_exclude_patterns.clear();
    let (mut browser, _) = FileBrowser::new(config);
    browser.search_index.home_dir = PathBuf::from("/home/user");
    let profile = file_index::IndexProfile::new("default", vec![PathBuf::from("/home/user")]);

    let _task = browser.accept_search_index_profile(Ok(Some(profile)));

    assert!(browser.search_index.exclude_pattern_inputs.is_empty());
    assert!(browser.user_config.search_index_exclude_patterns.is_empty());
}

#[test]
fn search_index_allowed_root_uses_loaded_home_directory() {
    let browser = browser_with_search_index_home();

    assert!(browser.search_index_root_is_allowed(Path::new("/home/user/Documents")));
    assert!(!browser.search_index_root_is_allowed(Path::new("/mnt/project")));
}

#[test]
fn simple_search_mode_does_not_start_index_status_or_build() {
    let mut browser = browser_with_search_index_home();
    browser.user_config.search_mode = crate::config::SearchBackendMode::Simple;
    browser.search_index.profile_roots = vec![PathBuf::from("/home/user")];

    let _status_task = browser.refresh_search_index_statuses();
    let _build_task = browser.request_search_index_manual_build(
        PathBuf::from("/home/user"),
        file_index::FileSearchIndexMode::FullRebuild,
    );

    assert!(browser.search_index.status_loading_roots.is_empty());
    assert!(browser.search_index.indexing_roots.is_empty());
}

#[test]
fn changing_selected_exclude_kind_turns_it_into_index_root() {
    let mut browser = browser_with_search_index_home();
    browser.search_index.exclude_pattern_inputs = vec!["/project/".to_owned()];
    browser.search_index.selected_path_rule = Some(SearchIndexPathRuleSelection::ExcludePattern(0));

    let _task = browser.change_search_index_path_rule_kind(
        SearchIndexPathRuleSelection::ExcludePattern(0),
        SearchIndexPathRuleKind::Indexed,
    );

    assert_eq!(
        browser.search_index.profile_roots,
        vec![
            PathBuf::from("/home/user"),
            PathBuf::from("/home/user/project"),
        ]
    );
    assert!(browser.search_index.exclude_pattern_inputs.is_empty());
    assert_eq!(browser.search_index.path_rule_input, "~/project");
    assert_eq!(
        browser.search_index.selected_path_rule,
        Some(SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from(
            "/home/user/project"
        )))
    );
    assert_eq!(
        browser.search_index.path_rule_kind,
        SearchIndexPathRuleKind::Indexed
    );
    assert_eq!(browser.search_index.path_rule_editor, None);
}

#[test]
fn selecting_path_rule_does_not_open_editor_until_modify_is_requested() {
    let (mut browser, _) = FileBrowser::new(crate::config::default_user_config());
    let selection = SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from("/home/user"));

    let _task = browser.select_search_index_path_rule(selection.clone());

    assert_eq!(browser.search_index.selected_path_rule, Some(selection));
    assert_eq!(browser.search_index.path_rule_editor, None);
}

#[test]
fn modify_button_first_opens_selected_path_rule_editor() {
    let mut browser = browser_with_search_index_home();
    browser.search_index.profile_roots = vec![PathBuf::from("/home/user/Documents")];
    let selection =
        SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from("/home/user/Documents"));
    browser.search_index.selected_path_rule = Some(selection.clone());

    let _task = browser.update_selected_search_index_path_rule();

    assert_eq!(
        browser.search_index.path_rule_editor,
        Some(SearchIndexPathRuleEditMode::Modifying(selection))
    );
    assert_eq!(browser.search_index.path_rule_input, "~/Documents");
    assert_eq!(
        browser.search_index.path_rule_kind,
        SearchIndexPathRuleKind::Indexed
    );
}

#[test]
fn add_button_first_opens_default_path_rule_editor() {
    let (mut browser, _) = FileBrowser::new(crate::config::default_user_config());

    let _task = browser.add_search_index_path_rule();

    assert_eq!(
        browser.search_index.path_rule_editor,
        Some(SearchIndexPathRuleEditMode::Adding)
    );
    assert_eq!(browser.search_index.path_rule_input, "~");
    assert_eq!(
        browser.search_index.path_rule_kind,
        SearchIndexPathRuleKind::Indexed
    );
}

#[test]
fn committing_add_editor_saves_path_rule_without_second_add_click() {
    let mut browser = browser_with_search_index_home();
    browser.search_index.profile_roots.clear();

    let _task = browser.add_search_index_path_rule();
    let _task = browser.update_search_index_path_rule_input("~/Documents".to_owned());
    let _task = browser.commit_search_index_path_rule_editor();

    assert_eq!(
        browser.search_index.profile_roots,
        vec![PathBuf::from("/home/user/Documents")]
    );
    assert_eq!(
        browser.search_index.selected_path_rule,
        Some(SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from(
            "/home/user/Documents"
        )))
    );
    assert_eq!(browser.search_index.path_rule_editor, None);
}

#[test]
fn committing_nested_root_keeps_it_when_home_is_already_indexed() {
    let mut browser = browser_with_search_index_home();

    let _task = browser.add_search_index_path_rule();
    let _task = browser.update_search_index_path_rule_input("~/Documents".to_owned());
    let _task = browser.commit_search_index_path_rule_editor();

    assert_eq!(
        browser.search_index.profile_roots,
        vec![
            PathBuf::from("/home/user"),
            PathBuf::from("/home/user/Documents"),
        ]
    );
    assert_eq!(
        browser.search_index.selected_path_rule,
        Some(SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from(
            "/home/user/Documents"
        )))
    );
}

#[test]
fn selecting_existing_row_commits_pending_add_before_selection_changes() {
    let mut browser = browser_with_search_index_home();
    browser.search_index.profile_roots = vec![PathBuf::from("/home/user/Downloads")];
    let existing = SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from("/home/user/Downloads"));

    let _task = browser.add_search_index_path_rule();
    let _task = browser.update_search_index_path_rule_input("~/Documents".to_owned());
    let _task = browser.select_search_index_path_rule(existing.clone());

    assert_eq!(
        browser.search_index.profile_roots,
        vec![
            PathBuf::from("/home/user/Downloads"),
            PathBuf::from("/home/user/Documents"),
        ]
    );
    assert_eq!(browser.search_index.selected_path_rule, Some(existing));
    assert_eq!(browser.search_index.path_rule_editor, None);
}

#[test]
fn same_kind_radio_click_does_not_select_or_edit_path_rule() {
    let mut browser = browser_with_search_index_home();
    browser.search_index.selected_path_rule = None;
    let selection = SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from("/home/user"));

    let _task =
        browser.change_search_index_path_rule_kind(selection, SearchIndexPathRuleKind::Indexed);

    assert_eq!(browser.search_index.selected_path_rule, None);
    assert_eq!(browser.search_index.path_rule_editor, None);
}

#[test]
fn exclude_radio_click_keeps_index_root_when_no_parent_exists() {
    let mut browser = browser_with_search_index_home();
    browser.search_index.selected_path_rule = None;
    let selection = SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from("/home/user"));

    let _task =
        browser.change_search_index_path_rule_kind(selection, SearchIndexPathRuleKind::Excluded);

    assert_eq!(
        browser.search_index.profile_roots,
        vec![PathBuf::from("/home/user")]
    );
    assert!(browser.search_index.exclude_pattern_inputs.is_empty());
    assert_eq!(browser.search_index.selected_path_rule, None);
    assert_eq!(browser.search_index.path_rule_editor, None);
    assert_eq!(
        browser.search_index.profile_error.as_deref(),
        Some("Add an indexed parent path before excluding this path.")
    );
}

#[test]
fn changing_rule_kind_does_not_open_path_editor() {
    let mut browser = browser_with_search_index_home();
    browser.search_index.exclude_pattern_inputs = vec!["/target/".to_owned()];
    let selection = SearchIndexPathRuleSelection::ExcludePattern(0);

    let _task =
        browser.change_search_index_path_rule_kind(selection, SearchIndexPathRuleKind::Indexed);

    assert_eq!(browser.search_index.path_rule_editor, None);
}

#[test]
fn radio_click_commits_pending_add_before_changing_clicked_rule_kind() {
    let mut browser = browser_with_search_index_home();
    browser.search_index.profile_roots = vec![PathBuf::from("/home/user/Project")];
    browser.search_index.exclude_pattern_inputs = vec!["/cache/".to_owned()];
    let existing = SearchIndexPathRuleSelection::ExcludePattern(0);

    let _task = browser.add_search_index_path_rule();
    let _task = browser.select_search_index_path_rule_kind(SearchIndexPathRuleKind::Excluded);
    let _task = browser.update_search_index_path_rule_input("~/Project/target".to_owned());
    let _task =
        browser.change_search_index_path_rule_kind(existing, SearchIndexPathRuleKind::Indexed);

    assert_eq!(
        browser.search_index.exclude_pattern_inputs,
        vec!["/target/".to_owned()]
    );
    assert_eq!(
        browser.search_index.profile_roots,
        vec![
            PathBuf::from("/home/user/Project"),
            PathBuf::from("/home/user/Project/cache"),
        ]
    );
    assert_eq!(browser.search_index.path_rule_editor, None);
}

#[test]
fn exclude_radio_click_keeps_row_position_in_path_rule_order() {
    let mut browser = browser_with_search_index_home();
    browser.search_index.profile_roots = vec![
        PathBuf::from("/home/user"),
        PathBuf::from("/home/user/Documents"),
    ];
    browser.search_index.exclude_pattern_inputs = vec!["node_modules/".to_owned()];
    browser
        .search_index
        .reset_path_rule_order_from_current_rules();
    let selection =
        SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from("/home/user/Documents"));

    let _task =
        browser.change_search_index_path_rule_kind(selection, SearchIndexPathRuleKind::Excluded);

    let entries = path_rule_selections(&browser);
    assert_eq!(
        entries,
        vec![
            SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from("/home/user")),
            SearchIndexPathRuleSelection::ExcludePattern(1),
            SearchIndexPathRuleSelection::ExcludePattern(0),
        ]
    );
    assert_eq!(
        browser.search_index.exclude_pattern_inputs,
        vec!["node_modules/".to_owned(), "/Documents/".to_owned()]
    );
}

#[test]
fn index_radio_click_keeps_row_position_after_profile_save_roundtrip() {
    let mut browser = browser_with_search_index_home();
    browser.search_index.profile_roots = vec![PathBuf::from("/home/user")];
    browser.search_index.exclude_pattern_inputs =
        vec!["/Project/cache/".to_owned(), "node_modules/".to_owned()];
    browser
        .search_index
        .reset_path_rule_order_from_current_rules();
    let selection = SearchIndexPathRuleSelection::ExcludePattern(0);

    let _task =
        browser.change_search_index_path_rule_kind(selection, SearchIndexPathRuleKind::Indexed);
    let mut saved_profile =
        file_index::IndexProfile::new("default", browser.search_index.profile_roots.clone());
    saved_profile.exclude_patterns = browser.search_index.exclude_pattern_inputs.clone();
    let _task = browser.accept_search_index_profile_save(Ok(saved_profile));

    assert_eq!(
        path_rule_selections(&browser),
        vec![
            SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from("/home/user")),
            SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from("/home/user/Project/cache")),
            SearchIndexPathRuleSelection::ExcludePattern(0),
        ]
    );
    assert_eq!(
        browser.search_index.exclude_pattern_inputs,
        vec!["node_modules/".to_owned()]
    );
}

#[test]
fn ignored_click_in_search_index_settings_commits_path_rule_editor() {
    let mut browser = browser_with_search_index_home();
    browser.search_index.profile_roots.clear();
    let settings_window = iced::window::Id::unique();
    browser.settings_window = Some(settings_window);
    browser.selected_settings_category = crate::model::SettingsCategory::SearchIndex;

    let _task = browser.add_search_index_path_rule();
    let _task = browser.update_search_index_path_rule_input("~/Documents".to_owned());
    let _task = browser.handle_window_pointer_pressed(
        settings_window,
        iced::mouse::Button::Left,
        iced::event::Status::Ignored,
    );

    assert_eq!(
        browser.search_index.profile_roots,
        vec![PathBuf::from("/home/user/Documents")]
    );
    assert_eq!(browser.search_index.path_rule_editor, None);
}

fn browser_with_search_index_home() -> FileBrowser {
    let (mut browser, _) = FileBrowser::new(crate::config::default_user_config());
    browser.current_dir = PathBuf::from("/mnt/project");
    browser.search_index.home_dir = PathBuf::from("/home/user");
    browser.search_index.profile_roots = vec![PathBuf::from("/home/user")];
    browser
}

fn search_index_roots_with_added_root(existing_roots: &[PathBuf], root: PathBuf) -> Vec<PathBuf> {
    if existing_roots.contains(&root) {
        return existing_roots.to_vec();
    }

    let mut roots = existing_roots.to_vec();
    roots.push(root);
    roots
}

fn path_rule_selections(browser: &FileBrowser) -> Vec<SearchIndexPathRuleSelection> {
    browser
        .search_index
        .path_rule_entries()
        .into_iter()
        .map(|entry| entry.selection)
        .collect()
}
