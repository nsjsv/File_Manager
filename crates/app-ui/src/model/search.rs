use file_search::{SearchHit, SearchQuery, SearchResultBatch};
use tokio_util::sync::CancellationToken;

use super::{SearchServiceDiagnostic, SearchServiceRecoveryAction, SearchServiceState};

pub(crate) const SEARCH_RESULT_WINDOW: usize = 100;

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
pub(crate) enum DirectoryFallbackCompletion {
    Completed,
    Cancelled,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchProvider {
    Indexed,
    DirectoryFallback,
}

#[derive(Debug, Clone)]
pub(crate) struct SearchState {
    pub(crate) input: String,
    pub(crate) active_query: Option<SearchQuery>,
    pub(crate) generation: u64,
    pub(crate) provider: Option<SearchProvider>,
    pub(crate) results: Vec<SearchHit>,
    pub(crate) is_loading: bool,
    pub(crate) indexed_batch_seen: bool,
    pub(crate) service: SearchServiceState,
    pub(crate) error: Option<String>,
    current_query_cancel: Option<CancellationToken>,
}

impl SearchState {
    pub(crate) fn new() -> Self {
        Self {
            input: String::new(),
            active_query: None,
            generation: 0,
            provider: None,
            results: Vec::new(),
            is_loading: false,
            indexed_batch_seen: false,
            service: SearchServiceState::new(),
            error: None,
            current_query_cancel: None,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        !self.input.is_empty()
            || self.active_query.is_some()
            || !self.results.is_empty()
            || self.is_loading
            || self.error.is_some()
    }

    pub(crate) fn begin_indexed_query(
        &mut self,
        generation: u64,
        query: SearchQuery,
    ) -> CancellationToken {
        self.cancel_current_query();
        let cancellation = CancellationToken::new();
        self.current_query_cancel = Some(cancellation.clone());
        self.generation = generation;
        self.active_query = Some(query);
        self.provider = Some(SearchProvider::Indexed);
        self.results.clear();
        self.is_loading = true;
        self.indexed_batch_seen = false;
        self.error = None;
        cancellation
    }

    pub(crate) fn accepts_indexed_outcome(&self, generation: u64) -> bool {
        generation == self.generation && self.provider == Some(SearchProvider::Indexed)
    }

    pub(crate) fn apply_indexed_batch(&mut self, mut batch: SearchResultBatch) {
        self.current_query_cancel = None;
        batch.hits.truncate(SEARCH_RESULT_WINDOW);
        self.results = batch.hits;
        self.indexed_batch_seen = true;
        self.is_loading = false;
        self.error = None;
    }

    pub(crate) fn apply_indexed_failure(&mut self, message: String) {
        self.current_query_cancel = None;
        if !self.indexed_batch_seen {
            self.results.clear();
        }
        self.is_loading = false;
        self.error = Some(message);
    }

    pub(crate) fn apply_indexed_cancellation(&mut self) {
        self.current_query_cancel = None;
        self.is_loading = false;
        self.error = None;
    }

    pub(crate) fn begin_directory_fallback(&mut self) -> CancellationToken {
        self.cancel_current_query();
        let cancellation = CancellationToken::new();
        self.current_query_cancel = Some(cancellation.clone());
        self.provider = Some(SearchProvider::DirectoryFallback);
        self.results.clear();
        self.is_loading = true;
        self.error = None;
        cancellation
    }

    pub(crate) fn accepts_directory_fallback(&self, generation: u64) -> bool {
        generation == self.generation && self.provider == Some(SearchProvider::DirectoryFallback)
    }

    pub(crate) fn apply_directory_batch(&mut self, mut hits: Vec<SearchHit>) {
        let remaining = SEARCH_RESULT_WINDOW.saturating_sub(self.results.len());
        hits.truncate(remaining);
        self.results.extend(hits);
        if self.results.len() == SEARCH_RESULT_WINDOW {
            if let Some(cancellation) = &self.current_query_cancel {
                cancellation.cancel();
            }
        }
        self.is_loading = false;
        self.error = None;
    }

    pub(crate) fn finish_directory_fallback(&mut self, completion: DirectoryFallbackCompletion) {
        self.current_query_cancel = None;
        self.is_loading = false;
        match completion {
            DirectoryFallbackCompletion::Completed | DirectoryFallbackCompletion::Cancelled => {
                self.error = None;
            }
            DirectoryFallbackCompletion::Failed(message) => {
                self.error = Some(message);
            }
        }
    }

    pub(crate) fn begin_service_restart(&mut self) -> Option<SearchServiceRecoveryAction> {
        self.service.begin_restart()
    }

    pub(crate) fn press_force_restart(&mut self) -> Option<SearchServiceRecoveryAction> {
        self.service.press_force_restart()
    }

    pub(crate) fn cancel_force_restart_confirmation(&mut self) -> bool {
        self.service.cancel_force_restart_confirmation()
    }

    pub(crate) fn accept_service_recovery_completion(
        &mut self,
        action: SearchServiceRecoveryAction,
        outcome: Result<file_search::SearchServiceStatus, SearchServiceDiagnostic>,
    ) -> bool {
        self.service.accept_recovery_completion(action, outcome)
    }

    pub(crate) fn abandon_query(&mut self) {
        self.cancel_current_query();
        self.generation = self.generation.saturating_add(1);
        self.clear_results();
    }

    pub(crate) fn abandon_and_clear_input(&mut self) {
        self.input.clear();
        self.abandon_query();
    }

    pub(crate) fn clear_results(&mut self) {
        self.active_query = None;
        self.provider = None;
        self.results.clear();
        self.is_loading = false;
        self.indexed_batch_seen = false;
        self.error = None;
    }

    fn cancel_current_query(&mut self) {
        if let Some(cancellation) = self.current_query_cancel.take() {
            cancellation.cancel();
        }
    }
}

impl Drop for SearchState {
    fn drop(&mut self) {
        self.cancel_current_query();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use file_search::{SearchQuery, SearchScope};

    use super::SearchState;
    use crate::model::{SearchServiceRecoveryAction, SearchServiceRecoveryState};

    fn directory_query(query_id: u64) -> SearchQuery {
        SearchQuery {
            query_id,
            terms: "report".to_owned(),
            scope: SearchScope::Directory(PathBuf::from("/workspace")),
            recursive: true,
            filters: Default::default(),
            limit: 100,
            cursor: None,
        }
    }

    #[test]
    fn service_diagnostic_does_not_activate_search_results_view() {
        let mut state = SearchState::new();
        state
            .service
            .observe_query_transport_failure("daemon exited");

        assert!(!state.is_active());
        assert!(state.error.is_none());
    }

    #[test]
    fn abandoning_query_cancels_directory_fallback() {
        let mut state = SearchState::new();
        state.input = "report".to_owned();
        state.begin_indexed_query(1, directory_query(1));
        let cancellation = state.begin_directory_fallback();

        state.abandon_and_clear_input();

        assert!(cancellation.is_cancelled());
        assert!(!state.is_active());
    }

    #[test]
    fn replacing_an_indexed_query_cancels_its_socket_work() {
        let mut state = SearchState::new();

        let first_cancellation = state.begin_indexed_query(1, directory_query(1));
        let second_cancellation = state.begin_indexed_query(2, directory_query(2));

        assert!(first_cancellation.is_cancelled());
        assert!(!second_cancellation.is_cancelled());
    }

    #[test]
    fn switching_provider_replaces_the_indexed_cancellation_token() {
        let mut state = SearchState::new();
        let indexed_cancellation = state.begin_indexed_query(1, directory_query(1));

        let fallback_cancellation = state.begin_directory_fallback();

        assert!(indexed_cancellation.is_cancelled());
        assert!(!fallback_cancellation.is_cancelled());
        state.abandon_query();
        assert!(fallback_cancellation.is_cancelled());
    }

    #[test]
    fn dropping_search_state_cancels_the_indexed_query() {
        let mut state = SearchState::new();
        let cancellation = state.begin_indexed_query(1, directory_query(1));

        drop(state);

        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn force_restart_requires_two_presses() {
        let mut state = SearchState::new();

        assert_eq!(state.press_force_restart(), None);
        assert_eq!(
            state.service.recovery,
            SearchServiceRecoveryState::ConfirmingForceRestart
        );
        assert_eq!(
            state.press_force_restart(),
            Some(SearchServiceRecoveryAction::ForceRestart)
        );
        assert_eq!(
            state.service.recovery,
            SearchServiceRecoveryState::Running(SearchServiceRecoveryAction::ForceRestart)
        );
    }

    #[test]
    fn running_recovery_blocks_every_second_submission() {
        let mut state = SearchState::new();

        assert_eq!(
            state.begin_service_restart(),
            Some(SearchServiceRecoveryAction::Restart)
        );
        assert_eq!(state.begin_service_restart(), None);
        assert_eq!(state.press_force_restart(), None);
        assert_eq!(
            state.service.recovery,
            SearchServiceRecoveryState::Running(SearchServiceRecoveryAction::Restart)
        );
    }

    #[test]
    fn force_restart_confirmation_can_be_cancelled_without_starting_recovery() {
        let mut state = SearchState::new();
        assert_eq!(state.press_force_restart(), None);

        assert!(state.cancel_force_restart_confirmation());

        assert_eq!(state.service.recovery, SearchServiceRecoveryState::Idle);
        assert!(!state.cancel_force_restart_confirmation());
    }
}
