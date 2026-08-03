use std::collections::HashSet;
use std::path::{Path, PathBuf};

use file_search::{SearchHit, SearchQuery, SearchResultBatch, SearchTextScope};
use tokio_util::sync::CancellationToken;

mod filters;
pub(crate) use filters::{
    SearchDateField, SearchDatePreset, SearchEntryTypePreset, SearchFilterPresetState,
};

pub(crate) const SEARCH_RESULT_WINDOW: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SearchWorkspaceSessionId(pub(crate) u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchInputStabilizationRequest {
    workspace_session_id: SearchWorkspaceSessionId,
    input_revision: u64,
    input: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum IndexedSearchOutcome {
    Batch(SearchResultBatch),
    Cancelled,
    TransportUnavailable(String),
    ProviderUnavailable(String),
    InvalidQuery(String),
    Fatal(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectoryFallbackOutcome {
    Completed(file_search::DirectoryFallbackCompletion),
    Cancelled,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchProvider {
    Indexed,
    DirectoryFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchResultCompletion {
    Complete,
    Truncated,
    Partial { inspected_entries: usize },
}

#[derive(Debug, Clone)]
pub(crate) struct SearchRootSnapshot {
    path: PathBuf,
}

impl SearchRootSnapshot {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SearchRunState {
    pub(crate) generation: u64,
    pub(crate) active_query: Option<SearchQuery>,
    pub(crate) provider: Option<SearchProvider>,
    pub(crate) indexed_batch_seen: bool,
    cancellation: Option<CancellationToken>,
}

impl SearchRunState {
    fn new() -> Self {
        Self {
            generation: 0,
            active_query: None,
            provider: None,
            indexed_batch_seen: false,
            cancellation: None,
        }
    }

    fn cancel(&mut self) {
        if let Some(cancellation) = self.cancellation.take() {
            cancellation.cancel();
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SearchResultWindow {
    pub(crate) hits: Vec<SearchHit>,
    pub(crate) is_loading: bool,
    pub(crate) failure: Option<String>,
    pub(crate) completion: Option<SearchResultCompletion>,
}

impl SearchResultWindow {
    fn new() -> Self {
        Self {
            hits: Vec::new(),
            is_loading: false,
            failure: None,
            completion: None,
        }
    }

    fn begin_loading(&mut self) {
        self.hits.clear();
        self.is_loading = true;
        self.failure = None;
        self.completion = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchSelectionGesture {
    Plain,
    Toggle,
    Range,
    AdditiveRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchSelectionStep {
    Previous,
    Next,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchKeyboardSelection {
    Replace,
    Extend,
}

#[derive(Debug, Clone)]
pub(crate) struct SearchResultSelection {
    selected_paths: HashSet<PathBuf>,
    anchor: Option<PathBuf>,
    focused: Option<PathBuf>,
}

impl SearchResultSelection {
    fn new() -> Self {
        Self {
            selected_paths: HashSet::new(),
            anchor: None,
            focused: None,
        }
    }

    pub(crate) fn is_selected(&self, path: &Path) -> bool {
        self.selected_paths.contains(path)
    }

    pub(crate) fn focused_path(&self) -> Option<&Path> {
        self.focused.as_deref()
    }

    pub(crate) fn selected_paths_in_result_order(&self, hits: &[SearchHit]) -> Vec<PathBuf> {
        hits.iter()
            .filter(|hit| self.selected_paths.contains(&hit.path))
            .map(|hit| hit.path.clone())
            .collect()
    }

    pub(crate) fn select(
        &mut self,
        hits: &[SearchHit],
        target: &Path,
        gesture: SearchSelectionGesture,
    ) {
        let Some(target_index) = hits.iter().position(|hit| hit.path == target) else {
            return;
        };
        match gesture {
            SearchSelectionGesture::Plain => {
                self.selected_paths.clear();
                self.selected_paths.insert(target.to_path_buf());
                self.anchor = Some(target.to_path_buf());
            }
            SearchSelectionGesture::Toggle => {
                if !self.selected_paths.remove(target) {
                    self.selected_paths.insert(target.to_path_buf());
                }
                self.anchor = Some(target.to_path_buf());
            }
            SearchSelectionGesture::Range | SearchSelectionGesture::AdditiveRange => {
                let anchor = self
                    .anchor
                    .as_deref()
                    .and_then(|path| hits.iter().position(|hit| hit.path == path))
                    .unwrap_or(target_index);
                if gesture == SearchSelectionGesture::Range {
                    self.selected_paths.clear();
                }
                let start = anchor.min(target_index);
                let end = anchor.max(target_index);
                self.selected_paths
                    .extend(hits[start..=end].iter().map(|hit| hit.path.clone()));
                self.anchor = Some(hits[anchor].path.clone());
            }
        }
        self.focused = Some(target.to_path_buf());
    }

    pub(crate) fn move_focus(
        &mut self,
        hits: &[SearchHit],
        step: SearchSelectionStep,
        keyboard_selection: SearchKeyboardSelection,
    ) -> Option<PathBuf> {
        if hits.is_empty() {
            return None;
        }
        let current_index = self
            .focused
            .as_deref()
            .and_then(|path| hits.iter().position(|hit| hit.path == path));
        let target_index = match (current_index, step) {
            (Some(index), SearchSelectionStep::Previous) => index.saturating_sub(1),
            (Some(index), SearchSelectionStep::Next) => (index + 1).min(hits.len() - 1),
            (None, SearchSelectionStep::Previous) => hits.len() - 1,
            (None, SearchSelectionStep::Next) => 0,
        };
        let target = hits[target_index].path.clone();
        let gesture = match keyboard_selection {
            SearchKeyboardSelection::Replace => SearchSelectionGesture::Plain,
            SearchKeyboardSelection::Extend => SearchSelectionGesture::Range,
        };
        self.select(hits, &target, gesture);
        Some(target)
    }

    pub(crate) fn select_all(&mut self, hits: &[SearchHit]) {
        self.selected_paths = hits.iter().map(|hit| hit.path.clone()).collect();
        self.anchor = hits.first().map(|hit| hit.path.clone());
        self.focused = hits.first().map(|hit| hit.path.clone());
    }

    fn clear(&mut self) {
        self.selected_paths.clear();
        self.anchor = None;
        self.focused = None;
    }

    fn reconcile(&mut self, hits: &[SearchHit]) {
        let visible_paths = hits
            .iter()
            .map(|hit| hit.path.clone())
            .collect::<HashSet<_>>();
        self.selected_paths
            .retain(|path| visible_paths.contains(path));
        if self
            .anchor
            .as_ref()
            .is_some_and(|path| !visible_paths.contains(path))
        {
            self.anchor = None;
        }
        if self
            .focused
            .as_ref()
            .is_some_and(|path| !visible_paths.contains(path))
        {
            self.focused = hits
                .iter()
                .find(|hit| self.selected_paths.contains(&hit.path))
                .map(|hit| hit.path.clone());
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SearchWorkspaceState {
    session_id: SearchWorkspaceSessionId,
    input_revision: u64,
    pub(crate) root: SearchRootSnapshot,
    pub(crate) input: String,
    pub(crate) filters: SearchFilterPresetState,
    pub(crate) run: SearchRunState,
    pub(crate) window: SearchResultWindow,
    pub(crate) selection: SearchResultSelection,
}

impl SearchWorkspaceState {
    pub(crate) fn new(root: PathBuf, session_id: SearchWorkspaceSessionId) -> Self {
        Self {
            session_id,
            input_revision: 0,
            root: SearchRootSnapshot::new(root),
            input: String::new(),
            filters: SearchFilterPresetState::default(),
            run: SearchRunState::new(),
            window: SearchResultWindow::new(),
            selection: SearchResultSelection::new(),
        }
    }

    pub(crate) fn next_generation(&self) -> u64 {
        self.run.generation.saturating_add(1)
    }

    pub(crate) fn replace_input(&mut self, input: String) -> SearchInputStabilizationRequest {
        self.input = input;
        self.input_revision = self.input_revision.wrapping_add(1);
        SearchInputStabilizationRequest {
            workspace_session_id: self.session_id,
            input_revision: self.input_revision,
            input: self.input.clone(),
        }
    }

    pub(crate) fn accepts_input_stabilization(
        &self,
        request: &SearchInputStabilizationRequest,
    ) -> bool {
        self.session_id == request.workspace_session_id
            && self.input_revision == request.input_revision
            && self.input == request.input
    }

    pub(crate) fn invalidate_input_stabilization(&mut self) {
        self.input_revision = self.input_revision.wrapping_add(1);
    }

    pub(crate) fn begin_indexed_query(&mut self, query: SearchQuery) -> CancellationToken {
        self.run.cancel();
        let cancellation = CancellationToken::new();
        self.run.cancellation = Some(cancellation.clone());
        self.run.generation = query.query_id;
        self.run.active_query = Some(query);
        self.run.provider = Some(SearchProvider::Indexed);
        self.run.indexed_batch_seen = false;
        self.window.begin_loading();
        self.selection.clear();
        cancellation
    }

    pub(crate) fn accepts_indexed_outcome(&self, generation: u64) -> bool {
        generation == self.run.generation && self.run.provider == Some(SearchProvider::Indexed)
    }

    pub(crate) fn apply_indexed_batch(&mut self, mut batch: SearchResultBatch) {
        self.run.cancellation = None;
        let provider_returned_extra = batch.hits.len() > SEARCH_RESULT_WINDOW;
        batch.hits.truncate(SEARCH_RESULT_WINDOW);
        self.window.hits = batch.hits;
        self.run.indexed_batch_seen = true;
        self.window.is_loading = false;
        self.window.failure = None;
        self.window.completion = Some(if provider_returned_extra || !batch.finished {
            SearchResultCompletion::Truncated
        } else {
            SearchResultCompletion::Complete
        });
        self.selection.reconcile(&self.window.hits);
    }

    pub(crate) fn apply_indexed_failure(&mut self, message: String) {
        self.run.cancellation = None;
        if !self.run.indexed_batch_seen {
            self.window.hits.clear();
            self.selection.clear();
        }
        self.window.is_loading = false;
        self.window.failure = Some(message);
    }

    pub(crate) fn apply_indexed_cancellation(&mut self) {
        self.run.cancellation = None;
        self.window.is_loading = false;
        self.window.failure = None;
    }

    pub(crate) fn begin_directory_fallback(&mut self) -> CancellationToken {
        self.run.cancel();
        let cancellation = CancellationToken::new();
        self.run.cancellation = Some(cancellation.clone());
        self.run.provider = Some(SearchProvider::DirectoryFallback);
        self.window.begin_loading();
        self.selection.clear();
        cancellation
    }

    pub(crate) fn accepts_directory_fallback(&self, generation: u64) -> bool {
        generation == self.run.generation
            && self.run.provider == Some(SearchProvider::DirectoryFallback)
    }

    pub(crate) fn apply_directory_batch(&mut self, hits: Vec<SearchHit>) {
        for hit in hits {
            if self.window.hits.len() == SEARCH_RESULT_WINDOW {
                self.window.completion = Some(SearchResultCompletion::Truncated);
                if let Some(cancellation) = &self.run.cancellation {
                    cancellation.cancel();
                }
                break;
            }
            self.window.hits.push(hit);
        }
        self.window.failure = None;
    }

    pub(crate) fn finish_directory_fallback(&mut self, outcome: DirectoryFallbackOutcome) {
        self.run.cancellation = None;
        self.window.is_loading = false;
        match outcome {
            DirectoryFallbackOutcome::Completed(completion) => {
                self.window.failure = None;
                if self.window.completion != Some(SearchResultCompletion::Truncated) {
                    self.window.completion = Some(match completion {
                        file_search::DirectoryFallbackCompletion::TraversalComplete { .. } => {
                            SearchResultCompletion::Complete
                        }
                        file_search::DirectoryFallbackCompletion::EntryBudgetReached {
                            inspected_entries,
                        } => SearchResultCompletion::Partial { inspected_entries },
                    });
                }
            }
            DirectoryFallbackOutcome::Cancelled => {
                self.window.failure = None;
            }
            DirectoryFallbackOutcome::Failed(message) => {
                self.window.failure = Some(message);
            }
        }
        self.selection.reconcile(&self.window.hits);
    }

    pub(crate) fn reject_query(&mut self, message: String) {
        self.run.cancel();
        self.run.generation = self.run.generation.saturating_add(1);
        self.run.active_query = None;
        self.run.provider = None;
        self.run.indexed_batch_seen = false;
        self.window.hits.clear();
        self.window.is_loading = false;
        self.window.failure = Some(message);
        self.window.completion = None;
        self.selection.clear();
    }

    pub(crate) fn selected_paths_in_result_order(&self) -> Vec<PathBuf> {
        self.selection
            .selected_paths_in_result_order(&self.window.hits)
    }

    pub(crate) fn hit_for_path(&self, path: &Path) -> Option<&SearchHit> {
        self.window.hits.iter().find(|hit| hit.path == path)
    }

    pub(crate) fn content_search_is_degraded(&self) -> bool {
        self.run.provider == Some(SearchProvider::DirectoryFallback)
            && self
                .run
                .active_query
                .as_ref()
                .is_some_and(|query| query.text_scope == SearchTextScope::NameAndContent)
    }
}

impl Drop for SearchWorkspaceState {
    fn drop(&mut self) {
        self.run.cancel();
    }
}

#[cfg(test)]
mod tests {
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
        let first = workspace.begin_indexed_query(directory_query(1, "first"));
        let second = workspace.begin_indexed_query(directory_query(2, "second"));

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
        workspace.begin_indexed_query(directory_query(1, "report"));
        workspace.begin_directory_fallback();
        assert!(workspace.content_search_is_degraded());

        let mut name_only = directory_query(2, "report");
        name_only.text_scope = SearchTextScope::NameOnly;
        workspace.begin_indexed_query(name_only);
        workspace.begin_directory_fallback();
        assert!(!workspace.content_search_is_degraded());
    }

    #[test]
    fn indexed_completion_distinguishes_exact_window_from_real_overflow() {
        let mut workspace =
            SearchWorkspaceState::new(PathBuf::from("/workspace"), SearchWorkspaceSessionId(1));
        workspace.begin_indexed_query(directory_query(1, ""));
        workspace.apply_indexed_batch(SearchResultBatch {
            query_id: 1,
            hits: (0..SEARCH_RESULT_WINDOW).map(hit).collect(),
            next_cursor: None,
            finished: true,
        });
        assert_eq!(
            workspace.window.completion,
            Some(SearchResultCompletion::Complete)
        );

        workspace.begin_indexed_query(directory_query(2, ""));
        workspace.apply_indexed_batch(SearchResultBatch {
            query_id: 2,
            hits: (0..SEARCH_RESULT_WINDOW).map(hit).collect(),
            next_cursor: Some(file_search::SearchCursor {
                offset: SEARCH_RESULT_WINDOW,
            }),
            finished: false,
        });
        assert_eq!(
            workspace.window.completion,
            Some(SearchResultCompletion::Truncated)
        );
    }

    #[test]
    fn fallback_needs_the_extra_hit_before_marking_the_window_truncated() {
        let mut workspace =
            SearchWorkspaceState::new(PathBuf::from("/workspace"), SearchWorkspaceSessionId(1));
        workspace.begin_indexed_query(directory_query(1, ""));
        let cancellation = workspace.begin_directory_fallback();
        workspace.apply_directory_batch((0..SEARCH_RESULT_WINDOW).map(hit).collect());

        assert_eq!(workspace.window.completion, None);
        assert!(!cancellation.is_cancelled());

        workspace.apply_directory_batch(vec![hit(SEARCH_RESULT_WINDOW)]);
        assert_eq!(
            workspace.window.completion,
            Some(SearchResultCompletion::Truncated)
        );
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn fallback_budget_completion_preserves_hits_as_partial_results() {
        let mut workspace =
            SearchWorkspaceState::new(PathBuf::from("/workspace"), SearchWorkspaceSessionId(1));
        workspace.begin_indexed_query(directory_query(1, "rare"));
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
}
