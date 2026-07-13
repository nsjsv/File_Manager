use file_search::{SearchHit, SearchQuery, SearchResultBatch, SearchServiceStatus};
use tokio_util::sync::CancellationToken;

pub(crate) const SEARCH_RESULT_WINDOW: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum IndexedSearchOutcome {
    Batch(SearchResultBatch),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SearchEndpointState {
    Starting,
    Connected(SearchServiceStatus),
    Unavailable { message: String },
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
    pub(crate) endpoint: SearchEndpointState,
    pub(crate) error: Option<String>,
    directory_fallback_cancel: Option<CancellationToken>,
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
            endpoint: SearchEndpointState::Starting,
            error: None,
            directory_fallback_cancel: None,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        !self.input.is_empty()
            || self.active_query.is_some()
            || !self.results.is_empty()
            || self.is_loading
            || self.error.is_some()
    }

    pub(crate) fn begin_indexed_query(&mut self, generation: u64, query: SearchQuery) {
        self.cancel_directory_fallback();
        self.generation = generation;
        self.active_query = Some(query);
        self.provider = Some(SearchProvider::Indexed);
        self.results.clear();
        self.is_loading = true;
        self.indexed_batch_seen = false;
        self.error = None;
    }

    pub(crate) fn accepts_indexed_outcome(&self, generation: u64) -> bool {
        generation == self.generation && self.provider == Some(SearchProvider::Indexed)
    }

    pub(crate) fn apply_indexed_batch(&mut self, mut batch: SearchResultBatch) {
        batch.hits.truncate(SEARCH_RESULT_WINDOW);
        self.results = batch.hits;
        self.indexed_batch_seen = true;
        self.is_loading = false;
        self.error = None;
    }

    pub(crate) fn apply_indexed_failure(&mut self, message: String) {
        if !self.indexed_batch_seen {
            self.results.clear();
        }
        self.is_loading = false;
        self.error = Some(message);
    }

    pub(crate) fn begin_directory_fallback(&mut self) -> CancellationToken {
        self.cancel_directory_fallback();
        let cancellation = CancellationToken::new();
        self.directory_fallback_cancel = Some(cancellation.clone());
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
            if let Some(cancellation) = &self.directory_fallback_cancel {
                cancellation.cancel();
            }
        }
        self.is_loading = false;
        self.error = None;
    }

    pub(crate) fn finish_directory_fallback(&mut self, completion: DirectoryFallbackCompletion) {
        self.directory_fallback_cancel = None;
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

    pub(crate) fn accept_endpoint_status(&mut self, status: SearchServiceStatus) {
        self.endpoint = SearchEndpointState::Connected(status);
    }

    pub(crate) fn accept_endpoint_failure(&mut self, message: String) {
        self.endpoint = SearchEndpointState::Unavailable { message };
    }

    pub(crate) fn abandon_query(&mut self) {
        self.cancel_directory_fallback();
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

    fn cancel_directory_fallback(&mut self) {
        if let Some(cancellation) = self.directory_fallback_cancel.take() {
            cancellation.cancel();
        }
    }
}

impl Drop for SearchState {
    fn drop(&mut self) {
        self.cancel_directory_fallback();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use file_search::{SearchQuery, SearchScope};

    use super::SearchState;

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
    fn endpoint_failure_does_not_activate_search_results_view() {
        let mut state = SearchState::new();
        state.accept_endpoint_failure("daemon exited".to_owned());

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
}
