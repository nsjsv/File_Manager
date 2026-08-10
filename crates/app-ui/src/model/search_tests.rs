use std::path::PathBuf;

use file_search::{
    MatchSource, SearchFileKind, SearchHit, SearchQuery, SearchResultBatch, SearchScope,
    SearchTextScope,
};

use super::*;

fn directory_query(query_id: u64, terms: &str) -> SearchQuery {
    SearchQuery {
        query_id,
        terms: terms.to_owned(),
        text_scope: SearchTextScope::NameAndContent,
        scope: SearchScope::Directory(PathBuf::from("/workspace")),
        recursive: true,
        filters: Default::default(),
        limit: SEARCH_RESULT_WINDOW,
        cursor: None,
    }
}

fn hit(index: usize) -> SearchHit {
    SearchHit {
        path: PathBuf::from(format!("/workspace/result-{index}.txt")),
        display_name: format!("result-{index}.txt"),
        kind: SearchFileKind::File,
        size: 0,
        modified_ms: None,
        accessed_ms: None,
        created_ms: None,
        rank: 1.0,
        snippet: None,
        match_source: MatchSource::Name,
    }
}

#[test]
fn workspace_root_is_frozen_while_empty_input_is_a_valid_state() {
    let workspace =
        SearchWorkspaceState::new(PathBuf::from("/workspace-a"), SearchWorkspaceSessionId(1));

    assert_eq!(workspace.root.path(), Path::new("/workspace-a"));
    assert!(workspace.input.is_empty());
}

#[test]
fn replacing_or_dropping_workspace_cancels_inflight_work() {
    let mut workspace =
        SearchWorkspaceState::new(PathBuf::from("/workspace"), SearchWorkspaceSessionId(1));
    let (_, first) = workspace.begin_indexed_query(directory_query(1, "first"));
    let (_, second) = workspace.begin_indexed_query(directory_query(2, "second"));

    assert!(first.is_cancelled());
    assert!(!second.is_cancelled());
    drop(workspace);
    assert!(second.is_cancelled());
}

#[test]
fn input_stabilization_accepts_only_the_current_revision() {
    let mut workspace =
        SearchWorkspaceState::new(PathBuf::from("/workspace"), SearchWorkspaceSessionId(1));
    let stale = workspace.replace_input("rep".to_owned());
    let current = workspace.replace_input("report".to_owned());

    assert!(!workspace.accepts_input_stabilization(&stale));
    assert!(workspace.accepts_input_stabilization(&current));

    workspace.invalidate_input_stabilization();
    assert!(!workspace.accepts_input_stabilization(&current));
}

#[test]
fn input_stabilization_cannot_cross_workspace_sessions() {
    let mut first =
        SearchWorkspaceState::new(PathBuf::from("/workspace"), SearchWorkspaceSessionId(1));
    let stale = first.replace_input("report".to_owned());
    let mut reopened =
        SearchWorkspaceState::new(PathBuf::from("/workspace"), SearchWorkspaceSessionId(2));
    let _ = reopened.replace_input("report".to_owned());

    assert!(!reopened.accepts_input_stabilization(&stale));
}

#[test]
fn content_degradation_is_derived_from_provider_and_active_text_scope() {
    let mut workspace =
        SearchWorkspaceState::new(PathBuf::from("/workspace"), SearchWorkspaceSessionId(1));
    let _ = workspace.begin_indexed_query(directory_query(1, "report"));
    workspace.begin_directory_fallback();
    assert!(workspace.content_search_is_degraded());

    let mut name_only = directory_query(2, "report");
    name_only.text_scope = SearchTextScope::NameOnly;
    let _ = workspace.begin_indexed_query(name_only);
    workspace.begin_directory_fallback();
    assert!(!workspace.content_search_is_degraded());
}

#[test]
fn indexed_completion_distinguishes_exact_window_from_real_overflow() {
    let mut workspace =
        SearchWorkspaceState::new(PathBuf::from("/workspace"), SearchWorkspaceSessionId(1));
    let (first_request, _) = workspace.begin_indexed_query(directory_query(1, ""));
    workspace.apply_indexed_batch(
        first_request,
        SearchResultBatch {
            query_id: 1,
            hits: (0..SEARCH_RESULT_WINDOW).map(hit).collect(),
            next_cursor: None,
            finished: true,
        },
    );
    assert_eq!(
        workspace.window.completion,
        Some(SearchResultCompletion::Complete)
    );

    let (second_request, _) = workspace.begin_indexed_query(directory_query(2, ""));
    workspace.apply_indexed_batch(
        second_request,
        SearchResultBatch {
            query_id: 2,
            hits: (0..SEARCH_RESULT_WINDOW).map(hit).collect(),
            next_cursor: Some(file_search::SearchCursor {
                offset: SEARCH_RESULT_WINDOW,
            }),
            finished: false,
        },
    );
    assert_eq!(
        workspace.window.completion,
        Some(SearchResultCompletion::MoreAvailable)
    );
}

#[test]
fn indexed_pages_append_once_and_advance_the_cursor() {
    let mut workspace =
        SearchWorkspaceState::new(PathBuf::from("/workspace"), SearchWorkspaceSessionId(1));
    let (first_request, _) = workspace.begin_indexed_query(directory_query(1, ""));
    workspace.apply_indexed_batch(
        first_request,
        SearchResultBatch {
            query_id: 1,
            hits: (0..SEARCH_RESULT_WINDOW).map(hit).collect(),
            next_cursor: Some(SearchCursor {
                offset: SEARCH_RESULT_WINDOW,
            }),
            finished: false,
        },
    );

    let (second_request, second_query, _) = workspace.begin_next_indexed_page().unwrap();
    assert_eq!(second_query.cursor, second_request.cursor);
    assert!(workspace.begin_next_indexed_page().is_none());
    assert!(workspace.accepts_indexed_outcome(second_request));
    workspace.apply_indexed_batch(
        second_request,
        SearchResultBatch {
            query_id: 1,
            hits: (SEARCH_RESULT_WINDOW..SEARCH_RESULT_WINDOW * 2)
                .map(hit)
                .collect(),
            next_cursor: None,
            finished: true,
        },
    );

    assert_eq!(workspace.window.hits.len(), SEARCH_RESULT_WINDOW * 2);
    assert_eq!(
        workspace.window.hits[SEARCH_RESULT_WINDOW].path,
        hit(SEARCH_RESULT_WINDOW).path
    );
    assert_eq!(
        workspace.window.completion,
        Some(SearchResultCompletion::Complete)
    );
    assert!(!workspace.accepts_indexed_outcome(second_request));
    assert!(workspace.begin_next_indexed_page().is_none());
}

#[test]
fn fallback_keeps_hits_beyond_the_indexed_page_size() {
    let mut workspace =
        SearchWorkspaceState::new(PathBuf::from("/workspace"), SearchWorkspaceSessionId(1));
    let _ = workspace.begin_indexed_query(directory_query(1, ""));
    let cancellation = workspace.begin_directory_fallback();
    workspace.apply_directory_batch((0..=SEARCH_RESULT_WINDOW).map(hit).collect());

    assert_eq!(workspace.window.hits.len(), SEARCH_RESULT_WINDOW + 1);
    assert_eq!(workspace.window.completion, None);
    assert!(!cancellation.is_cancelled());
}

#[test]
fn fallback_budget_completion_preserves_hits_as_partial_results() {
    let mut workspace =
        SearchWorkspaceState::new(PathBuf::from("/workspace"), SearchWorkspaceSessionId(1));
    let _ = workspace.begin_indexed_query(directory_query(1, "rare"));
    workspace.begin_directory_fallback();
    workspace.apply_directory_batch(vec![hit(1)]);
    workspace.finish_directory_fallback(DirectoryFallbackOutcome::Completed(
        file_search::DirectoryFallbackCompletion::EntryBudgetReached {
            inspected_entries: 50_000,
        },
    ));

    assert_eq!(workspace.window.hits.len(), 1);
    assert_eq!(
        workspace.window.completion,
        Some(SearchResultCompletion::Partial {
            inspected_entries: 50_000,
        })
    );
}

#[test]
fn search_selection_supports_plain_toggle_range_and_result_order() {
    let hits = (0..5).map(hit).collect::<Vec<_>>();
    let mut selection = SearchResultSelection::new();
    selection.select(&hits, &hits[1].path, SearchSelectionGesture::Plain);
    selection.select(&hits, &hits[3].path, SearchSelectionGesture::Range);
    assert_eq!(
        selection.selected_paths_in_result_order(&hits),
        hits[1..=3]
            .iter()
            .map(|hit| hit.path.clone())
            .collect::<Vec<_>>()
    );

    selection.select(&hits, &hits[2].path, SearchSelectionGesture::Toggle);
    assert!(!selection.is_selected(&hits[2].path));
    selection.select(&hits, &hits[4].path, SearchSelectionGesture::AdditiveRange);
    assert!(selection.is_selected(&hits[4].path));
}

#[test]
fn keyboard_selection_moves_focus_and_select_all_uses_result_order() {
    let hits = (0..3).map(hit).collect::<Vec<_>>();
    let mut selection = SearchResultSelection::new();
    assert_eq!(
        selection.move_focus(
            &hits,
            SearchSelectionStep::Next,
            SearchKeyboardSelection::Replace,
        ),
        Some(hits[0].path.clone())
    );
    assert_eq!(
        selection.move_focus(
            &hits,
            SearchSelectionStep::Next,
            SearchKeyboardSelection::Extend,
        ),
        Some(hits[1].path.clone())
    );
    assert_eq!(selection.selected_paths_in_result_order(&hits).len(), 2);

    selection.select_all(&hits);
    assert_eq!(selection.selected_paths_in_result_order(&hits).len(), 3);
}
