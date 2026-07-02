use std::path::PathBuf;

use file_core::FileKind;
use file_index::{FileSearchMatch, FileSearchOutcome, SearchResultSource};

use super::*;

#[test]
fn selected_search_match_offset_clamps_first_item_to_start() {
    assert_eq!(selected_search_match_offset(0, 30), 0.0);
}

#[test]
fn selected_search_match_offset_centers_middle_item() {
    let index = 10;
    let offset = selected_search_match_offset(index, 30);
    let row_pitch = SEARCH_RESULT_ROW_HEIGHT + SEARCH_RESULT_ROW_SPACING;
    let selected_center =
        SEARCH_RESULTS_PADDING + index as f32 * row_pitch + SEARCH_RESULT_ROW_HEIGHT / 2.0;

    assert!((selected_center - offset - SEARCH_RESULTS_HEIGHT / 2.0).abs() < 0.01);
}

#[test]
fn selected_search_match_offset_clamps_last_item_to_end() {
    let match_count = 30;
    let offset = selected_search_match_offset(match_count - 1, match_count);
    let max_offset = (search_results_content_height(match_count) - SEARCH_RESULTS_HEIGHT).max(0.0);

    assert_eq!(offset, max_offset);
}

#[test]
fn search_request_generation_rejects_repeated_stale_query() {
    let search = search_state_for_request_generation(2);
    let stale_request = request_with_search_generation(&search, 1);

    assert!(!search_request_matches_active_state(
        &search,
        &stale_request
    ));
    assert!(search_request_matches_active_state(
        &search,
        &search.request()
    ));
}

#[test]
fn stale_search_input_stabilization_keeps_search_idle() {
    let search = search_state_for_request_generation(2);
    let stale_request = request_with_search_generation(&search, 1);
    let mut browser = browser_with_search_state(search);

    let _command = browser.load_stable_search_matches(stale_request);

    let search = browser.search.as_ref().expect("search state remains open");
    assert!(!search.is_loading);
    assert!(search.matches.is_empty());
}

#[test]
fn stale_search_matches_loaded_keeps_current_matches() {
    let mut search = search_state_for_request_generation(2);
    search.is_loading = true;
    search.matches = vec![search_match_at_path("/tmp/current")];
    search.selected_match = Some(0);
    search.skipped_count = 3;
    let stale_request = request_with_search_generation(&search, 1);
    let mut browser = browser_with_search_state(search);
    let stale_outcome = FileSearchOutcome {
        root: PathBuf::from("/tmp"),
        matches: vec![search_match_at_path("/tmp/stale")],
        skipped: Vec::new(),
    };

    let _command = browser.accept_search_matches(stale_request, Ok(stale_outcome));

    let search = browser.search.as_ref().expect("search state remains open");
    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.matches[0].path, PathBuf::from("/tmp/current"));
    assert_eq!(search.selected_match, Some(0));
    assert_eq!(search.skipped_count, 3);
    assert!(search.is_loading);
}

#[test]
fn empty_search_query_clears_results_and_advances_generation() {
    let mut search = search_state_for_request_generation(7);
    search.is_loading = true;
    search.matches = vec![search_match_at_path("/tmp/current")];
    search.selected_match = Some(0);
    search.skipped_count = 2;
    search.error = Some("previous search failed".to_owned());
    let mut browser = browser_with_search_state(search);

    let _command = browser.update_search_query("  ".to_owned());

    let search = browser.search.as_ref().expect("search state remains open");
    assert_eq!(search.request_generation, 8);
    assert_eq!(search.query, "  ");
    assert!(search.matches.is_empty());
    assert_eq!(search.selected_match, None);
    assert_eq!(search.skipped_count, 0);
    assert_eq!(search.error, None);
    assert!(!search.is_loading);
}

#[test]
fn simple_mode_rejects_content_media_search_modes() {
    let mut search = search_state_for_request_generation(4);
    search.matches = vec![search_match_at_path("/tmp/current")];
    search.selected_match = Some(0);
    let mut browser = browser_with_search_state(search);

    let _command = browser.select_search_mode(SearchMode::Contents);

    let search = browser.search.as_ref().expect("search state remains open");
    assert_eq!(search.mode, SearchMode::Files);
    assert_eq!(search.request_generation, 4);
    assert_eq!(search.matches.len(), 1);
}

#[test]
fn indexed_mode_selecting_search_mode_advances_generation_and_clears_results() {
    let mut search = search_state_for_request_generation(4);
    search.matches = vec![search_match_at_path("/tmp/current")];
    search.selected_match = Some(0);
    search.skipped_count = 2;
    let mut browser = browser_with_search_state(search);
    browser.user_config.search_mode = crate::config::SearchBackendMode::Indexed;

    let _command = browser.select_search_mode(SearchMode::Contents);

    let search = browser.search.as_ref().expect("search state remains open");
    assert_eq!(search.mode, SearchMode::Contents);
    assert_eq!(search.request_generation, 5);
    assert!(search.matches.is_empty());
    assert_eq!(search.selected_match, None);
    assert_eq!(search.skipped_count, 0);
}

#[test]
fn simple_mode_search_uses_tree_search_without_indexing() {
    let mut browser = browser_with_search_state(search_state_for_request_generation(1));
    browser.search_index.statuses.insert(
        PathBuf::from("/tmp"),
        search_index_status(PathBuf::from("/tmp"), true, true),
    );

    let _command = browser.load_search_matches();

    let search = browser.search.as_ref().expect("search state remains open");
    assert!(search.is_loading);
    assert!(search.search_cancel.is_some());
    assert!(!search.is_indexing);
    assert!(!browser
        .search_index
        .indexing_roots
        .contains(&PathBuf::from("/tmp")));
}

#[test]
fn indexed_files_search_stale_index_rebuilds_and_uses_tree_fallback() {
    let mut browser = browser_with_search_state(search_state_for_request_generation(1));
    browser.user_config.search_mode = crate::config::SearchBackendMode::Indexed;
    browser.search_index.home_dir = PathBuf::from("/tmp");
    browser.search_index.statuses.insert(
        PathBuf::from("/tmp"),
        search_index_status(PathBuf::from("/tmp"), true, true),
    );

    let _command = browser.load_search_matches();

    let search = browser.search.as_ref().expect("search state remains open");
    assert!(search.is_loading);
    assert!(search.search_cancel.is_some());
    assert!(browser
        .search_index
        .indexing_roots
        .contains(&PathBuf::from("/tmp")));
}

#[test]
fn indexed_contents_search_stale_index_waits_for_rebuild_without_tree_fallback() {
    let mut search = search_state_for_request_generation(1);
    search.mode = SearchMode::Contents;
    let mut browser = browser_with_search_state(search);
    browser.user_config.search_mode = crate::config::SearchBackendMode::Indexed;
    browser.search_index.home_dir = PathBuf::from("/tmp");
    browser.search_index.statuses.insert(
        PathBuf::from("/tmp"),
        search_index_status(PathBuf::from("/tmp"), true, true),
    );

    let _command = browser.load_search_matches();

    let search = browser.search.as_ref().expect("search state remains open");
    assert!(search.is_loading);
    assert!(search.search_cancel.is_none());
    assert!(browser
        .search_index
        .indexing_roots
        .contains(&PathBuf::from("/tmp")));
}

#[test]
fn indexed_ready_search_keeps_cancel_token_for_superseding_queries() {
    let mut browser = browser_with_search_state(search_state_for_request_generation(1));
    browser.user_config.search_mode = crate::config::SearchBackendMode::Indexed;
    browser.search_index.statuses.insert(
        PathBuf::from("/tmp"),
        search_index_status(PathBuf::from("/tmp"), true, false),
    );

    let _command = browser.load_search_matches();

    let search = browser.search.as_ref().expect("search state remains open");
    assert!(search.is_loading);
    assert!(search.search_cancel.is_some());
}

fn search_state_for_request_generation(request_generation: u64) -> SearchState {
    SearchState {
        scope: SearchScope::CurrentDirectory,
        mode: SearchMode::Files,
        root: PathBuf::from("/tmp"),
        query: "needle".to_owned(),
        request_generation,
        search_cancel: None,
        matches: Vec::new(),
        selected_match: None,
        is_loading: false,
        is_indexing: false,
        skipped_count: 0,
        error: None,
        index_error: None,
    }
}

fn browser_with_search_state(search: SearchState) -> FileBrowser {
    let (mut browser, _) = FileBrowser::new(crate::config::default_user_config());
    browser.search_index.base_dir = PathBuf::from("/tmp/file-manager-search-test-index");
    browser.search = Some(search);
    browser
}

fn request_with_search_generation(search: &SearchState, generation: u64) -> SearchRequest {
    SearchRequest {
        scope: search.scope,
        mode: search.mode,
        root: search.root.clone(),
        query: search.query.clone(),
        generation,
    }
}

fn search_match_at_path(path: &str) -> FileSearchMatch {
    let path = PathBuf::from(path);
    let name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("match"))
        .to_os_string();
    FileSearchMatch {
        relative_path: PathBuf::from(&name),
        path,
        name,
        kind: FileKind::File,
        rank_score: 1,
        source: SearchResultSource::Files,
        snippet: None,
        media: None,
    }
}

fn search_index_status(
    root: PathBuf,
    exists: bool,
    stale: bool,
) -> file_index::FileSearchIndexStatus {
    file_index::FileSearchIndexStatus {
        root,
        index_dir: PathBuf::from("/tmp/index"),
        exists,
        stale,
        reason: stale.then(|| "search index content policy is outdated".to_owned()),
        include_hidden: false,
        content_index_enabled: false,
        content_max_file_bytes: 16 * 1024 * 1024,
        media_metadata_scope: file_index::MediaMetadataScope::Off,
        record_count: 0,
        index_size_bytes: 0,
        built_at_ms: None,
        updated_at_ms: None,
        failed_count: 0,
        exclude_rules_hash: None,
        extractor_version: None,
        failures: Vec::new(),
    }
}
