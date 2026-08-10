use std::collections::HashSet;
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use iced::Point;

use file_search::{MatchSource, SearchFileKind, SearchHit, SearchResultBatch, SearchScope};
use tempfile::tempdir;

use super::FileBrowser;
use crate::config;
use crate::model::search::{SearchProvider, SEARCH_RESULT_WINDOW};
use crate::model::{
    ContextMenuState, DirectoryFallbackOutcome, IndexedSearchOutcome, IndexedSearchRequest,
    SearchEntryTypeMenuState, SearchEntryTypePreset, SearchResultCompletion,
    SearchServiceDiagnosticKind, SelectionMarquee, SelectionMarqueePhase,
    SelectionMarqueeScrollAnchor, SelectionMarqueeSource,
};

fn browser_for_search_tests(root: PathBuf) -> FileBrowser {
    let root = if root.is_dir() {
        root
    } else {
        tempdir().unwrap().keep()
    };
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    browser.current_dir = root.clone();
    browser.directory_collection_phase = crate::model::DirectoryCollectionPhase::Ready;
    let pane_id = browser.active_pane_id();
    browser.pane_by_id_mut(pane_id).unwrap().current_dir = root;
    browser
}

fn search_hit(path: PathBuf, kind: SearchFileKind) -> SearchHit {
    SearchHit {
        display_name: path
            .file_name()
            .expect("search hit path should contain a file name")
            .to_string_lossy()
            .into_owned(),
        path,
        kind,
        size: 0,
        modified_ms: None,
        accessed_ms: None,
        created_ms: None,
        rank: 1.0,
        snippet: None,
        match_source: MatchSource::Name,
    }
}

fn indexed_batch(query_id: u64, hits: Vec<SearchHit>, finished: bool) -> SearchResultBatch {
    SearchResultBatch {
        query_id,
        hits,
        next_cursor: (!finished).then_some(file_search::SearchCursor {
            offset: SEARCH_RESULT_WINDOW,
        }),
        finished,
    }
}

fn pending_indexed_request(browser: &FileBrowser) -> IndexedSearchRequest {
    browser
        .search_workspace
        .as_ref()
        .unwrap()
        .run
        .pending_indexed_request
        .expect("pending indexed request")
}

fn stabilize_search_input(browser: &mut FileBrowser, value: &str) {
    if browser.search_workspace.is_none() {
        drop(browser.submit_search());
    }
    let request = browser
        .search_workspace
        .as_mut()
        .unwrap()
        .replace_input(value.to_owned());
    drop(browser.accept_search_input_stabilization(request));
}

#[test]
fn opening_search_workspace_clears_file_view_pointer_interactions() {
    let mut browser = browser_for_search_tests(PathBuf::from("/workspace"));
    let pane_id = browser.active_pane_id();
    browser.selection_marquee = Some(SelectionMarquee {
        gesture_origin: Point::new(10.0, 10.0),
        start: Point::new(10.0, 10.0),
        current: Point::new(40.0, 40.0),
        source: SelectionMarqueeSource::PaneBlank,
        phase: SelectionMarqueePhase::Selecting,
        scroll_anchor: SelectionMarqueeScrollAnchor::List {
            pane_id,
            offset_y: 0.0,
        },
        base_selection: HashSet::new(),
        preserve_existing: false,
    });
    browser.drag_selection_anchor = Some(browser.current_dir.join("anchor.txt"));

    drop(browser.submit_search());

    assert!(browser.search_workspace.is_some());
    assert!(browser.selection_marquee.is_none());
    assert!(browser.drag_selection_anchor.is_none());
}

#[test]
fn service_status_is_independent_from_search_workspace_lifecycle() {
    let mut browser = browser_for_search_tests(PathBuf::from("/workspace"));
    browser
        .search_service
        .observe_query_transport_failure("daemon exited");

    assert!(browser.search_workspace.is_none());
    assert_eq!(
        browser.search_service.incidents[0].kind,
        SearchServiceDiagnosticKind::EndpointUnavailable
    );
}

#[test]
fn workspace_freezes_root_and_empty_input_runs_a_query() {
    let mut browser = browser_for_search_tests(PathBuf::from("/workspace-a"));
    let frozen_root = browser.current_dir.clone();
    drop(browser.submit_search());
    browser.current_dir = tempdir().unwrap().keep();
    drop(browser.update_search_input("later".to_owned()));

    let workspace = browser.search_workspace.as_ref().unwrap();
    assert_eq!(workspace.root.path(), frozen_root);
    assert_eq!(workspace.input, "later");
    assert!(matches!(
        workspace.run.active_query.as_ref().unwrap().scope,
        SearchScope::Directory(ref root) if root == &frozen_root
    ));
}

#[test]
fn indexed_window_truth_comes_from_provider_completion() {
    let mut browser = browser_for_search_tests(PathBuf::from("/workspace"));
    drop(browser.submit_search());
    let request = pending_indexed_request(&browser);
    let generation = request.generation;
    let hits = (0..SEARCH_RESULT_WINDOW)
        .map(|index| {
            search_hit(
                PathBuf::from(format!("/workspace/result-{index}.txt")),
                SearchFileKind::File,
            )
        })
        .collect();
    drop(browser.accept_search_results(
        request,
        IndexedSearchOutcome::Batch(indexed_batch(generation, hits, true)),
    ));

    assert_eq!(
        browser.search_workspace.as_ref().unwrap().window.completion,
        Some(SearchResultCompletion::Complete)
    );
}

#[test]
fn approaching_loaded_end_requests_and_appends_one_indexed_page() {
    let mut browser = browser_for_search_tests(PathBuf::from("/workspace"));
    drop(browser.submit_search());
    let first_request = pending_indexed_request(&browser);
    let generation = first_request.generation;
    let first_hits = (0..SEARCH_RESULT_WINDOW)
        .map(|index| {
            search_hit(
                PathBuf::from(format!("/workspace/result-{index}.txt")),
                SearchFileKind::File,
            )
        })
        .collect();
    drop(browser.accept_search_results(
        first_request,
        IndexedSearchOutcome::Batch(indexed_batch(generation, first_hits, false)),
    ));

    let viewport_height = crate::view::SEARCH_RESULT_ROW_HEIGHT * 4.0;
    let offset_y = crate::view::SEARCH_RESULT_ROW_HEIGHT * 90.0;
    drop(browser.update_search_results_viewport(offset_y, viewport_height));
    let next_request = pending_indexed_request(&browser);
    assert_eq!(
        next_request.cursor,
        Some(file_search::SearchCursor {
            offset: SEARCH_RESULT_WINDOW,
        })
    );
    drop(browser.update_search_results_viewport(offset_y, viewport_height));
    assert_eq!(pending_indexed_request(&browser), next_request);

    let second_hits = (SEARCH_RESULT_WINDOW..SEARCH_RESULT_WINDOW * 2)
        .map(|index| {
            search_hit(
                PathBuf::from(format!("/workspace/result-{index}.txt")),
                SearchFileKind::File,
            )
        })
        .collect();
    drop(browser.accept_search_results(
        next_request,
        IndexedSearchOutcome::Batch(indexed_batch(generation, second_hits, true)),
    ));

    let workspace = browser.search_workspace.as_ref().unwrap();
    assert_eq!(workspace.window.hits.len(), SEARCH_RESULT_WINDOW * 2);
    assert_eq!(
        workspace.window.completion,
        Some(SearchResultCompletion::Complete)
    );
    assert!(workspace.run.pending_indexed_request.is_none());
}

#[test]
fn unavailable_before_first_batch_switches_to_directory_fallback() {
    let mut browser = browser_for_search_tests(PathBuf::from("/workspace"));
    stabilize_search_input(&mut browser, "report");
    let request = pending_indexed_request(&browser);

    drop(browser.accept_search_results(
        request,
        IndexedSearchOutcome::ProviderUnavailable("index is starting".to_owned()),
    ));

    let workspace = browser.search_workspace.as_ref().unwrap();
    assert_eq!(
        workspace.run.provider,
        Some(SearchProvider::DirectoryFallback)
    );
    assert!(workspace.window.is_loading);
    assert!(workspace.window.failure.is_none());
}

#[test]
fn unavailable_after_indexed_batch_does_not_mix_providers() {
    let mut browser = browser_for_search_tests(PathBuf::from("/workspace"));
    stabilize_search_input(&mut browser, "report");
    drop(browser.submit_search());
    let request = browser
        .search_workspace
        .as_ref()
        .unwrap()
        .run
        .pending_indexed_request
        .expect("new indexed request");
    let generation = request.generation;
    drop(browser.accept_search_results(
        request,
        IndexedSearchOutcome::Batch(indexed_batch(
            generation,
            vec![search_hit(
                PathBuf::from("/workspace/report.txt"),
                SearchFileKind::File,
            )],
            false,
        )),
    ));
    drop(browser.update_search_results_viewport(f32::MAX, crate::view::SEARCH_RESULT_ROW_HEIGHT));
    let next_request = browser
        .search_workspace
        .as_ref()
        .unwrap()
        .run
        .pending_indexed_request
        .expect("next indexed request");
    drop(browser.accept_search_results(
        next_request,
        IndexedSearchOutcome::TransportUnavailable("socket closed".to_owned()),
    ));

    let workspace = browser.search_workspace.as_ref().unwrap();
    assert_eq!(workspace.run.provider, Some(SearchProvider::Indexed));
    assert_eq!(workspace.window.hits.len(), 1);
    assert_eq!(workspace.window.failure.as_deref(), Some("socket closed"));
}

#[test]
fn fallback_budget_and_overflow_have_distinct_truthful_states() {
    let mut browser = browser_for_search_tests(PathBuf::from("/workspace"));
    drop(browser.submit_search());
    let request = pending_indexed_request(&browser);
    let generation = request.generation;
    drop(browser.accept_search_results(
        request,
        IndexedSearchOutcome::ProviderUnavailable("not ready".to_owned()),
    ));
    drop(browser.accept_directory_search_batch(
        generation,
        vec![search_hit(
            PathBuf::from("/workspace/partial.txt"),
            SearchFileKind::File,
        )],
    ));
    drop(browser.accept_directory_search_finished(
        generation,
        DirectoryFallbackOutcome::Completed(
            file_search::DirectoryFallbackCompletion::EntryBudgetReached {
                inspected_entries: 50_000,
            },
        ),
    ));
    assert_eq!(
        browser.search_workspace.as_ref().unwrap().window.completion,
        Some(SearchResultCompletion::Partial {
            inspected_entries: 50_000,
        })
    );

    drop(browser.submit_search());
    let request = pending_indexed_request(&browser);
    let generation = request.generation;
    drop(browser.accept_search_results(
        request,
        IndexedSearchOutcome::ProviderUnavailable("not ready".to_owned()),
    ));
    let hits = (0..=SEARCH_RESULT_WINDOW)
        .map(|index| {
            search_hit(
                PathBuf::from(format!("/workspace/result-{index}.txt")),
                SearchFileKind::File,
            )
        })
        .collect();
    drop(browser.accept_directory_search_batch(generation, hits));
    assert_eq!(
        browser.search_workspace.as_ref().unwrap().window.hits.len(),
        SEARCH_RESULT_WINDOW + 1
    );
}

#[test]
fn unavailable_frozen_root_cancels_the_run_without_switching_scope() {
    let root = tempdir().unwrap().keep();
    let mut browser = browser_for_search_tests(root.clone());
    drop(browser.submit_search());
    let first_generation = browser.search_workspace.as_ref().unwrap().run.generation;
    std::fs::remove_dir(&root).unwrap();

    let request = browser
        .search_workspace
        .as_mut()
        .unwrap()
        .replace_input("later".to_owned());
    drop(browser.accept_search_input_stabilization(request));

    let workspace = browser.search_workspace.as_ref().unwrap();
    assert_eq!(workspace.root.path(), root);
    assert!(workspace.run.generation > first_generation);
    assert!(workspace.run.active_query.is_none());
    assert!(workspace.window.hits.is_empty());
    assert!(workspace
        .window
        .failure
        .as_deref()
        .is_some_and(|message| message.starts_with("Search root is unavailable:")));
}

#[test]
fn stale_generation_cannot_pollute_restarted_or_closed_workspace() {
    let mut browser = browser_for_search_tests(PathBuf::from("/workspace"));
    stabilize_search_input(&mut browser, "first");
    let stale = pending_indexed_request(&browser);
    stabilize_search_input(&mut browser, "second");
    drop(browser.accept_search_results(
        stale,
        IndexedSearchOutcome::Batch(indexed_batch(
            stale.generation,
            vec![search_hit(
                PathBuf::from("/workspace/stale.txt"),
                SearchFileKind::File,
            )],
            true,
        )),
    ));
    assert!(browser
        .search_workspace
        .as_ref()
        .unwrap()
        .window
        .hits
        .is_empty());

    drop(browser.close_search_workspace());
    drop(browser.accept_search_results(
        stale,
        IndexedSearchOutcome::Batch(indexed_batch(stale.generation, Vec::new(), true)),
    ));
    assert!(browser.search_workspace.is_none());
}

#[test]
fn immediate_search_actions_invalidate_pending_input_stabilization() {
    let mut browser = browser_for_search_tests(PathBuf::from("/workspace"));
    drop(browser.submit_search());

    let submit_request = browser
        .search_workspace
        .as_mut()
        .unwrap()
        .replace_input("submit".to_owned());
    drop(browser.submit_search());
    let submit_generation = browser.search_workspace.as_ref().unwrap().run.generation;
    drop(browser.accept_search_input_stabilization(submit_request));
    assert_eq!(
        browser.search_workspace.as_ref().unwrap().run.generation,
        submit_generation
    );

    let filter_request = browser
        .search_workspace
        .as_mut()
        .unwrap()
        .replace_input("filter".to_owned());
    drop(browser.toggle_search_entry_type(SearchEntryTypePreset::Images));
    let filter_generation = browser.search_workspace.as_ref().unwrap().run.generation;
    drop(browser.accept_search_input_stabilization(filter_request));
    assert_eq!(
        browser.search_workspace.as_ref().unwrap().run.generation,
        filter_generation
    );

    let reset_request = browser
        .search_workspace
        .as_mut()
        .unwrap()
        .replace_input("reset".to_owned());
    drop(browser.reset_search_filters());
    let reset_generation = browser.search_workspace.as_ref().unwrap().run.generation;
    drop(browser.accept_search_input_stabilization(reset_request));
    assert_eq!(
        browser.search_workspace.as_ref().unwrap().run.generation,
        reset_generation
    );
}

#[test]
fn reopening_workspace_rejects_old_session_and_clears_type_menu() {
    let mut browser = browser_for_search_tests(PathBuf::from("/workspace"));
    drop(browser.submit_search());
    let stale = browser
        .search_workspace
        .as_mut()
        .unwrap()
        .replace_input("old session".to_owned());
    browser.context_menu = Some(ContextMenuState::SearchEntryTypes(
        SearchEntryTypeMenuState {
            position: iced::Point::ORIGIN,
        },
    ));

    drop(browser.close_search_workspace());
    assert!(browser.context_menu.is_none());
    drop(browser.submit_search());
    let reopened_generation = browser.search_workspace.as_ref().unwrap().run.generation;
    drop(browser.accept_search_input_stabilization(stale));

    assert_eq!(
        browser.search_workspace.as_ref().unwrap().run.generation,
        reopened_generation
    );
}

#[test]
fn search_selection_does_not_mutate_browser_selection() {
    let mut browser = browser_for_search_tests(PathBuf::from("/workspace"));
    browser
        .selected_paths
        .insert(PathBuf::from("/workspace/browser.txt"));
    drop(browser.submit_search());
    let request = pending_indexed_request(&browser);
    let generation = request.generation;
    let first = PathBuf::from("/workspace/first.txt");
    let second = PathBuf::from("/workspace/second.txt");
    drop(browser.accept_search_results(
        request,
        IndexedSearchOutcome::Batch(indexed_batch(
            generation,
            vec![
                search_hit(first.clone(), SearchFileKind::File),
                search_hit(second.clone(), SearchFileKind::File),
            ],
            true,
        )),
    ));
    drop(browser.press_search_result(first.clone()));
    browser.keyboard_modifiers = iced::keyboard::Modifiers::CTRL;
    drop(browser.press_search_result(second.clone()));

    assert_eq!(
        browser.active_search_selection().unwrap(),
        vec![first, second]
    );
    assert_eq!(browser.selected_paths.len(), 1);
    assert!(browser
        .selected_paths
        .contains(Path::new("/workspace/browser.txt")));
}

#[test]
fn opening_containing_directory_closes_workspace_and_reveals_result() {
    let root = tempdir().unwrap();
    let parent = root.path().join("folder");
    std::fs::create_dir(&parent).unwrap();
    let path = parent.join("report.txt");
    std::fs::write(&path, "report").unwrap();
    let mut browser = browser_for_search_tests(root.path().to_path_buf());
    drop(browser.submit_search());

    drop(browser.open_search_containing_directory(path.clone()));

    assert!(browser.search_workspace.is_none());
    assert_eq!(browser.current_dir, parent);
    assert_eq!(browser.selected.as_ref(), Some(&path));
    assert!(browser.selected_paths.contains(&path));
}

#[test]
fn missing_containing_directory_keeps_workspace_and_reports_failure() {
    let root = tempdir().unwrap();
    let path = root.path().join("missing/report.txt");
    let mut browser = browser_for_search_tests(root.path().to_path_buf());
    drop(browser.submit_search());

    drop(browser.open_search_containing_directory(path));

    let workspace = browser.search_workspace.as_ref().unwrap();
    assert!(workspace
        .window
        .failure
        .as_deref()
        .is_some_and(|message| { message.contains("Containing directory is unavailable") }));
}

#[cfg(unix)]
#[test]
fn activating_non_utf8_directory_preserves_native_path_and_closes_workspace() {
    let root = tempdir().unwrap();
    let path = root
        .path()
        .join(OsString::from_vec(b"directory-\x80".to_vec()));
    std::fs::create_dir(&path).unwrap();
    let mut browser = browser_for_search_tests(root.path().to_path_buf());
    drop(browser.submit_search());
    let request = pending_indexed_request(&browser);
    let generation = request.generation;
    drop(browser.accept_search_results(
        request,
        IndexedSearchOutcome::Batch(indexed_batch(
            generation,
            vec![search_hit(path.clone(), SearchFileKind::Directory)],
            true,
        )),
    ));

    drop(browser.activate_search_path(path.clone()));

    assert_eq!(browser.current_dir, path);
    assert!(browser.search_workspace.is_none());
}
