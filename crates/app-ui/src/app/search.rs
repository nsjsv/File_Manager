use std::path::PathBuf;

use file_search::{SearchExcludeRules, SearchHit, SearchQuery, SearchScope};
use iced::Task;

use super::FileBrowser;
use crate::commands::{
    directory_fallback_search_command, open_file_command, search_command,
    search_service_recovery_command, search_service_status_command,
};
use crate::model::search::SEARCH_RESULT_WINDOW;
use crate::model::{
    DirectoryFallbackCompletion, IndexedSearchOutcome, Message, NavigationMode,
    SearchServiceRecoveryAction,
};

impl FileBrowser {
    pub(super) fn update_search_input(&mut self, value: String) -> Task<Message> {
        self.search.input = value;
        if self.search.input.trim().is_empty() {
            self.search.abandon_query();
            return Task::none();
        }
        self.submit_search()
    }

    pub(super) fn submit_search(&mut self) -> Task<Message> {
        let terms = self.search.input.trim().to_owned();
        if terms.is_empty() {
            self.search.abandon_query();
            return Task::none();
        }
        let generation = self.search.generation.saturating_add(1);
        let query = SearchQuery {
            query_id: generation,
            terms,
            scope: SearchScope::Directory(self.current_search_directory()),
            recursive: true,
            filters: Default::default(),
            limit: SEARCH_RESULT_WINDOW,
            cursor: None,
        };
        let cancellation = self.search.begin_indexed_query(generation, query.clone());
        search_command(generation, query, cancellation)
    }

    pub(super) fn accept_search_results(
        &mut self,
        generation: u64,
        outcome: IndexedSearchOutcome,
    ) -> Task<Message> {
        if !self.search.accepts_indexed_outcome(generation) {
            return Task::none();
        }
        match outcome {
            IndexedSearchOutcome::Cancelled => {
                self.search.apply_indexed_cancellation();
            }
            IndexedSearchOutcome::Batch(batch) => {
                self.search.apply_indexed_batch(batch);
            }
            IndexedSearchOutcome::TransportUnavailable(message) => {
                self.search.accept_endpoint_failure(message.clone());
                return self.switch_to_directory_fallback(message);
            }
            IndexedSearchOutcome::ProviderUnavailable(message) => {
                return self.switch_to_directory_fallback(message);
            }
            IndexedSearchOutcome::InvalidQuery(message) | IndexedSearchOutcome::Fatal(message) => {
                self.search.apply_indexed_failure(message);
            }
        }
        Task::none()
    }

    pub(super) fn accept_directory_search_batch(
        &mut self,
        generation: u64,
        hits: Vec<SearchHit>,
    ) -> Task<Message> {
        if self.search.accepts_directory_fallback(generation) {
            self.search.apply_directory_batch(hits);
        }
        Task::none()
    }

    pub(super) fn accept_directory_search_finished(
        &mut self,
        generation: u64,
        completion: DirectoryFallbackCompletion,
    ) -> Task<Message> {
        if self.search.accepts_directory_fallback(generation) {
            self.search.finish_directory_fallback(completion);
        }
        Task::none()
    }

    pub(super) fn activate_search_hit(&mut self, hit: SearchHit) -> Task<Message> {
        if hit.kind == file_search::SearchFileKind::Directory {
            self.navigate_to(hit.path, NavigationMode::RecordHistory)
        } else {
            open_file_command(hit.path, self.terminal_emulator)
        }
    }

    pub(super) fn clear_search(&mut self) -> Task<Message> {
        self.search.abandon_and_clear_input();
        Task::none()
    }

    pub(super) fn restart_search_service(&mut self) -> Task<Message> {
        self.search
            .begin_service_restart()
            .map(search_service_recovery_command)
            .unwrap_or_else(Task::none)
    }

    pub(super) fn press_force_restart_search_service(&mut self) -> Task<Message> {
        self.search
            .press_force_restart()
            .map(search_service_recovery_command)
            .unwrap_or_else(Task::none)
    }

    pub(super) fn accept_search_service_recovery(
        &mut self,
        action: SearchServiceRecoveryAction,
        outcome: Result<file_search::SearchServiceStatus, String>,
    ) -> Task<Message> {
        let completion = outcome.map(|_| ());
        if self
            .search
            .accept_service_recovery_completion(action, completion)
        {
            search_service_status_command()
        } else {
            Task::none()
        }
    }

    pub(super) fn toggle_search_content_indexing(&mut self) -> Task<Message> {
        self.user_config.search_content_indexing_enabled =
            !self.user_config.search_content_indexing_enabled;
        self.persist_app_config_command()
    }

    fn switch_to_directory_fallback(&mut self, unavailable_message: String) -> Task<Message> {
        if self.search.indexed_batch_seen {
            self.search.apply_indexed_failure(unavailable_message);
            return Task::none();
        }

        let Some(query) = self.search.active_query.clone() else {
            self.search.apply_indexed_failure(unavailable_message);
            return Task::none();
        };
        if !query.recursive {
            self.search.apply_indexed_failure(unavailable_message);
            return Task::none();
        }
        let SearchScope::Directory(directory) = &query.scope else {
            self.search.apply_indexed_failure(unavailable_message);
            return Task::none();
        };
        if query.cursor.is_some() || self.path_is_remote_mount(directory) {
            self.search.apply_indexed_failure(unavailable_message);
            return Task::none();
        }

        let generation = self.search.generation;
        let cancellation = self.search.begin_directory_fallback();
        directory_fallback_search_command(
            generation,
            query,
            SearchExcludeRules::new(Vec::new()),
            cancellation,
        )
    }

    fn current_search_directory(&self) -> PathBuf {
        self.pane_by_id(self.active_pane_id())
            .map(|pane| pane.current_dir.clone())
            .unwrap_or_else(|| self.current_dir.clone())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;

    use desktop_linux::{
        MountedNetworkConnection, NetworkConnection, NetworkConnectionId, NetworkProtocol,
    };
    use file_search::{
        MatchSource, SearchCursor, SearchFileKind, SearchHit, SearchQuery, SearchResultBatch,
    };

    use super::FileBrowser;
    use crate::config;
    use crate::model::search::{
        SearchProvider, SearchServiceRecoveryAction, SearchServiceRecoveryState,
        SEARCH_RESULT_WINDOW,
    };
    use crate::model::{DirectoryFallbackCompletion, IndexedSearchOutcome, SettingsCategory};

    fn browser_for_search_tests() -> FileBrowser {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        browser.current_dir = PathBuf::from("/workspace");
        browser.is_loading = false;
        browser
    }

    fn search_hit(path: &str) -> SearchHit {
        SearchHit {
            path: PathBuf::from(path),
            display_name: PathBuf::from(path)
                .file_name()
                .expect("search hit path should contain a file name")
                .to_string_lossy()
                .into_owned(),
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
    fn indexed_search_keeps_only_the_first_result_window() {
        let mut browser = browser_for_search_tests();
        browser.search.input = "report".to_owned();

        drop(browser.submit_search());
        let generation = browser.search.generation;

        drop(browser.accept_search_results(
            generation,
            IndexedSearchOutcome::Batch(SearchResultBatch {
                query_id: generation,
                hits: vec![search_hit("/workspace/report-1.txt")],
                next_cursor: Some(SearchCursor { offset: 1 }),
                finished: false,
            }),
        ));

        assert_eq!(browser.search.results.len(), 1);
        assert_eq!(
            browser.search.results[0].path,
            PathBuf::from("/workspace/report-1.txt")
        );
        assert!(!browser.search.is_loading);
    }

    #[cfg(unix)]
    #[test]
    fn activating_non_utf8_directory_hit_preserves_the_native_path() {
        let mut browser = browser_for_search_tests();
        let path = PathBuf::from(OsString::from_vec(b"/workspace/\x80".to_vec()));
        let mut hit = search_hit("/workspace/placeholder");
        hit.path = path.clone();
        hit.kind = SearchFileKind::Directory;

        drop(browser.activate_search_hit(hit));

        assert_eq!(browser.current_dir, path);
    }

    #[test]
    fn oversized_indexed_batch_is_truncated_at_the_result_window() {
        let mut browser = browser_for_search_tests();
        browser.search.input = "report".to_owned();

        drop(browser.submit_search());
        let generation = browser.search.generation;
        let hits = (0..=SEARCH_RESULT_WINDOW)
            .map(|index| search_hit(&format!("/workspace/report-{index}.txt")))
            .collect();

        drop(browser.accept_search_results(
            generation,
            IndexedSearchOutcome::Batch(SearchResultBatch {
                query_id: generation,
                hits,
                next_cursor: Some(SearchCursor { offset: 1 }),
                finished: false,
            }),
        ));

        assert_eq!(browser.search.results.len(), SEARCH_RESULT_WINDOW);
    }

    #[test]
    fn unavailable_before_first_batch_switches_to_directory_fallback() {
        let mut browser = browser_for_search_tests();
        browser.search.input = "report".to_owned();
        drop(browser.submit_search());
        let generation = browser.search.generation;

        drop(browser.accept_search_results(
            generation,
            IndexedSearchOutcome::ProviderUnavailable("index is starting".to_owned()),
        ));

        assert_eq!(
            browser.search.provider,
            Some(SearchProvider::DirectoryFallback)
        );
        assert!(browser.search.is_loading);
        assert!(browser.search.error.is_none());
    }

    #[test]
    fn unavailable_after_indexed_batch_does_not_mix_providers() {
        let mut browser = browser_for_search_tests();
        browser.search.input = "report".to_owned();
        drop(browser.submit_search());
        let generation = browser.search.generation;

        drop(browser.accept_search_results(
            generation,
            IndexedSearchOutcome::Batch(SearchResultBatch {
                query_id: generation,
                hits: vec![search_hit("/workspace/report-1.txt")],
                next_cursor: Some(SearchCursor { offset: 1 }),
                finished: false,
            }),
        ));
        drop(browser.accept_search_results(
            generation,
            IndexedSearchOutcome::TransportUnavailable("socket closed".to_owned()),
        ));

        assert_eq!(browser.search.provider, Some(SearchProvider::Indexed));
        assert_eq!(browser.search.results.len(), 1);
        assert_eq!(browser.search.error.as_deref(), Some("socket closed"));
    }

    #[test]
    fn invalid_query_does_not_start_fallback() {
        let mut browser = browser_for_search_tests();
        browser.search.input = "report".to_owned();
        drop(browser.submit_search());
        let generation = browser.search.generation;

        drop(browser.accept_search_results(
            generation,
            IndexedSearchOutcome::InvalidQuery("bad filter".to_owned()),
        ));

        assert_eq!(browser.search.provider, Some(SearchProvider::Indexed));
        assert_eq!(browser.search.error.as_deref(), Some("bad filter"));
    }

    #[test]
    fn global_scope_does_not_start_directory_fallback() {
        let mut browser = browser_for_search_tests();
        let generation = 1;
        browser
            .search
            .begin_indexed_query(generation, SearchQuery::global(generation, "report"));

        drop(browser.accept_search_results(
            generation,
            IndexedSearchOutcome::ProviderUnavailable("index is unavailable".to_owned()),
        ));

        assert_eq!(browser.search.provider, Some(SearchProvider::Indexed));
        assert_eq!(
            browser.search.error.as_deref(),
            Some("index is unavailable")
        );
    }

    #[test]
    fn mounted_network_scope_does_not_start_directory_fallback() {
        let mut browser = browser_for_search_tests();
        let connection = NetworkConnection::new(
            NetworkConnectionId::new("nas"),
            "NAS",
            NetworkProtocol::Smb,
            "smb://server/share",
        )
        .unwrap();
        browser.network_connections =
            crate::network_connections::NetworkConnectionState::from_connections(vec![
                connection.clone()
            ]);
        browser
            .network_connections
            .accept_mounted(MountedNetworkConnection {
                connection,
                mount_path: PathBuf::from("/run/user/1000/gvfs/nas"),
            });
        let generation = 1;
        browser.search.begin_indexed_query(
            generation,
            SearchQuery {
                query_id: generation,
                terms: "report".to_owned(),
                scope: file_search::SearchScope::Directory(PathBuf::from(
                    "/run/user/1000/gvfs/nas/reports",
                )),
                recursive: true,
                filters: Default::default(),
                limit: 100,
                cursor: None,
            },
        );

        drop(browser.accept_search_results(
            generation,
            IndexedSearchOutcome::ProviderUnavailable("index is unavailable".to_owned()),
        ));

        assert_eq!(browser.search.provider, Some(SearchProvider::Indexed));
        assert_eq!(
            browser.search.error.as_deref(),
            Some("index is unavailable")
        );
    }

    #[test]
    fn fallback_batches_append_and_completion_finishes_search() {
        let mut browser = browser_for_search_tests();
        browser.search.input = "report".to_owned();
        drop(browser.submit_search());
        let generation = browser.search.generation;
        drop(browser.accept_search_results(
            generation,
            IndexedSearchOutcome::ProviderUnavailable("index is starting".to_owned()),
        ));

        drop(browser.accept_directory_search_batch(
            generation,
            vec![search_hit("/workspace/report-1.txt")],
        ));
        drop(browser.accept_directory_search_batch(
            generation,
            vec![search_hit("/workspace/report-2.txt")],
        ));
        drop(
            browser.accept_directory_search_finished(
                generation,
                DirectoryFallbackCompletion::Completed,
            ),
        );

        assert_eq!(browser.search.results.len(), 2);
        assert!(!browser.search.is_loading);
        assert!(browser.search.error.is_none());
    }

    #[test]
    fn fallback_result_window_cancels_the_directory_walk() {
        let mut browser = browser_for_search_tests();
        browser.search.input = "report".to_owned();
        drop(browser.submit_search());
        let generation = browser.search.generation;
        let cancellation = browser.search.begin_directory_fallback();
        let hits = (0..=SEARCH_RESULT_WINDOW)
            .map(|index| search_hit(&format!("/workspace/report-{index}.txt")))
            .collect();

        drop(browser.accept_directory_search_batch(generation, hits));

        assert_eq!(browser.search.results.len(), SEARCH_RESULT_WINDOW);
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn stale_generation_does_not_pollute_current_results() {
        let mut browser = browser_for_search_tests();
        browser.search.input = "first".to_owned();
        drop(browser.submit_search());
        let stale_generation = browser.search.generation;

        browser.search.input = "second".to_owned();
        drop(browser.submit_search());
        let current_generation = browser.search.generation;

        drop(browser.accept_search_results(
            stale_generation,
            IndexedSearchOutcome::Batch(SearchResultBatch {
                query_id: stale_generation,
                hits: vec![search_hit("/workspace/stale.txt")],
                next_cursor: None,
                finished: true,
            }),
        ));
        assert!(browser.search.results.is_empty());

        drop(browser.accept_search_results(
            current_generation,
            IndexedSearchOutcome::Batch(SearchResultBatch {
                query_id: current_generation,
                hits: vec![search_hit("/workspace/current.txt")],
                next_cursor: None,
                finished: true,
            }),
        ));
        assert_eq!(browser.search.results.len(), 1);
    }

    #[test]
    fn clearing_search_invalidates_inflight_generation() {
        let mut browser = browser_for_search_tests();
        browser.search.input = "report".to_owned();
        drop(browser.submit_search());
        let stale_generation = browser.search.generation;

        drop(browser.clear_search());
        drop(browser.accept_search_results(
            stale_generation,
            IndexedSearchOutcome::Batch(SearchResultBatch {
                query_id: stale_generation,
                hits: vec![search_hit("/workspace/stale.txt")],
                next_cursor: None,
                finished: true,
            }),
        ));

        assert!(browser.search.results.is_empty());
        assert!(browser.search.input.is_empty());
        assert!(browser.search.active_query.is_none());
        assert!(!browser.search.is_active());
    }

    #[test]
    fn navigating_away_cancels_directory_fallback() {
        let mut browser = browser_for_search_tests();
        browser.search.input = "report".to_owned();
        drop(browser.submit_search());
        let cancellation = browser.search.begin_directory_fallback();

        drop(browser.navigate_to(
            PathBuf::from("/workspace/other"),
            crate::model::NavigationMode::RecordHistory,
        ));

        assert!(cancellation.is_cancelled());
        assert!(browser.search.input.is_empty());
        assert!(!browser.search.is_active());
    }

    #[test]
    fn settings_escape_cancels_force_confirmation_before_closing_the_window() {
        let mut browser = browser_for_search_tests();
        browser.selected_settings_category = SettingsCategory::Search;
        drop(browser.ensure_settings_window());
        drop(browser.press_force_restart_search_service());

        drop(browser.handle_focused_window_escape_pressed());

        assert_eq!(browser.search.recovery, SearchServiceRecoveryState::Idle);
        assert!(browser.settings_window.is_some());
    }

    #[test]
    fn leaving_search_settings_cancels_only_a_pending_confirmation() {
        let mut browser = browser_for_search_tests();
        browser.selected_settings_category = SettingsCategory::Search;
        drop(browser.press_force_restart_search_service());

        drop(browser.select_settings_category(SettingsCategory::General));

        assert_eq!(browser.search.recovery, SearchServiceRecoveryState::Idle);
        assert_eq!(
            browser.selected_settings_category,
            SettingsCategory::General
        );
    }

    #[test]
    fn closing_settings_cancels_a_pending_force_confirmation() {
        let mut browser = browser_for_search_tests();
        drop(browser.ensure_settings_window());
        drop(browser.press_force_restart_search_service());

        drop(browser.close_settings_window());

        assert_eq!(browser.search.recovery, SearchServiceRecoveryState::Idle);
        assert!(browser.settings_window.is_none());
    }

    #[test]
    fn closing_settings_does_not_cancel_running_recovery() {
        let mut browser = browser_for_search_tests();
        drop(browser.ensure_settings_window());
        assert_eq!(
            browser.search.begin_service_restart(),
            Some(SearchServiceRecoveryAction::Restart)
        );

        drop(browser.close_settings_window());

        assert_eq!(
            browser.search.recovery,
            SearchServiceRecoveryState::Running(SearchServiceRecoveryAction::Restart)
        );
        assert!(browser.settings_window.is_none());
    }
}
