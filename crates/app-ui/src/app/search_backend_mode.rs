use iced::Task;

use super::FileBrowser;
use crate::config::{SearchBackendMode, SearchModePromptStatus};
use crate::model::{Message, SearchIndexDaemonStatus, SearchMode, SearchModePromptState};

impl FileBrowser {
    pub(super) fn refresh_search_mode_prompt(&mut self) -> Task<Message> {
        if self.user_config.search_mode_prompt == SearchModePromptStatus::Pending {
            if self.search_mode_prompt.is_none() {
                self.search_mode_prompt = Some(SearchModePromptState::default());
            }
        } else {
            self.search_mode_prompt = None;
        }
        Task::none()
    }

    pub(super) fn select_search_mode_prompt_mode(
        &mut self,
        mode: SearchBackendMode,
    ) -> Task<Message> {
        if let Some(prompt) = &mut self.search_mode_prompt {
            prompt.selected_mode = Some(mode);
        }
        Task::none()
    }

    pub(super) fn accept_search_mode_prompt(&mut self) -> Task<Message> {
        match self
            .search_mode_prompt
            .as_ref()
            .and_then(|prompt| prompt.selected_mode)
        {
            Some(SearchBackendMode::Simple) => self.select_simple_search_from_prompt(),
            Some(SearchBackendMode::Indexed) => self.select_indexed_search_from_prompt(),
            None => Task::none(),
        }
    }

    pub(super) fn select_simple_search_from_prompt(&mut self) -> Task<Message> {
        self.search_mode_prompt = None;
        self.user_config.search_mode = SearchBackendMode::Simple;
        self.user_config.search_mode_prompt = SearchModePromptStatus::Completed;
        self.search_index.service_generation = self.search_index.service_generation.wrapping_add(1);
        self.clear_indexed_search_state();
        self.persist_user_preferences_command()
    }

    pub(super) fn select_indexed_search_from_prompt(&mut self) -> Task<Message> {
        self.search_mode_prompt = None;
        self.user_config.search_mode = SearchBackendMode::Indexed;
        self.search_index.service_generation = self.search_index.service_generation.wrapping_add(1);
        self.start_indexed_search_bootstrap()
    }

    pub(super) fn select_search_backend_mode(&mut self, mode: SearchBackendMode) -> Task<Message> {
        if self.user_config.search_mode == mode
            && self.user_config.search_mode_prompt == SearchModePromptStatus::Completed
        {
            return Task::none();
        }

        self.user_config.search_mode = mode;
        self.user_config.search_mode_prompt = SearchModePromptStatus::Completed;
        self.search_mode_prompt = None;
        self.search_index.service_generation = self.search_index.service_generation.wrapping_add(1);

        match mode {
            SearchBackendMode::Simple => {
                self.clear_indexed_search_state();
                self.persist_user_preferences_command()
            }
            SearchBackendMode::Indexed => Task::batch([
                self.persist_user_preferences_command(),
                self.start_indexed_search_bootstrap(),
            ]),
        }
    }

    pub(crate) fn search_backend_mode(&self) -> SearchBackendMode {
        self.user_config.search_mode
    }

    pub(crate) fn effective_search_backend_mode(&self) -> SearchBackendMode {
        if self.indexed_search_ready() {
            SearchBackendMode::Indexed
        } else {
            SearchBackendMode::Simple
        }
    }

    pub(crate) fn indexed_search_ready(&self) -> bool {
        self.user_config.search_mode == SearchBackendMode::Indexed
            && matches!(
                self.search_index.daemon_status,
                Some(SearchIndexDaemonStatus::Reachable)
            )
            && !self.search_index.daemon_status_loading
            && !self.search_index.profile_loading
            && self.search_index.has_active_profile_roots()
    }

    pub(super) fn start_indexed_search_bootstrap(&mut self) -> Task<Message> {
        if self.user_config.search_mode != SearchBackendMode::Indexed {
            return Task::none();
        }

        self.reset_indexed_runtime_for_bootstrap();
        self.request_indexed_search_bootstrap()
    }

    pub(super) fn request_indexed_search_bootstrap(&mut self) -> Task<Message> {
        if self.user_config.search_mode != SearchBackendMode::Indexed {
            return Task::none();
        }

        self.search_index.bootstrap_in_progress = true;
        self.mark_open_search_as_indexed_fallback_session();
        self.refresh_search_index_daemon_status()
    }

    pub(super) fn promote_fallback_search_reopen_hint_if_ready(&mut self) {
        if !self.indexed_search_ready() {
            return;
        }

        let Some(search) = &mut self.search else {
            return;
        };
        if search.indexed_fallback_session
            && search.session_backend_mode == SearchBackendMode::Simple
        {
            search.indexed_fallback_session = false;
            search.show_index_ready_reopen_hint = true;
        }
    }

    pub(super) fn clear_open_search_indexed_fallback_notice(&mut self) {
        let Some(search) = &mut self.search else {
            return;
        };
        if search.session_backend_mode == SearchBackendMode::Simple {
            search.indexed_fallback_session = false;
        }
    }

    fn reset_indexed_runtime_for_bootstrap(&mut self) {
        self.startup_index_setup = None;
        self.search_index.bootstrap_in_progress = false;
        self.search_index.profile_loading = false;
        self.search_index.profile_error = None;
        self.search_index.daemon_status = None;
        self.search_index.daemon_status_loading = false;
        self.search_index.profile_roots.clear();
        self.search_index.statuses.clear();
        self.search_index.root_errors.clear();
        self.search_index.status_loading_roots.clear();
        self.search_index.indexing_roots.clear();
        self.search_index.pending_startup_index_builds.clear();
    }

    fn clear_indexed_search_state(&mut self) {
        self.reset_indexed_runtime_for_bootstrap();
        if let Some(search) = &mut self.search {
            if search.mode != SearchMode::Files {
                search.mode = SearchMode::Files;
                search.request_generation = search.request_generation.wrapping_add(1);
                search.matches.clear();
                search.selected_match = None;
                search.skipped_count = 0;
            }
            search.session_backend_mode = SearchBackendMode::Simple;
            search.indexed_fallback_session = false;
            search.show_index_ready_reopen_hint = false;
            search.is_indexing = false;
            search.index_error = None;
        }
    }

    fn mark_open_search_as_indexed_fallback_session(&mut self) {
        let Some(search) = &mut self.search else {
            return;
        };
        if search.session_backend_mode == SearchBackendMode::Simple {
            search.indexed_fallback_session = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config;
    use crate::model::{
        SearchMode, SearchScope, SearchState, SidebarLocation, SidebarLocationKind,
    };

    #[test]
    fn selecting_simple_from_prompt_completes_prompt_without_index_setup() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        browser.search_mode_prompt = Some(SearchModePromptState {
            selected_mode: Some(SearchBackendMode::Simple),
        });

        let _task = browser.accept_search_mode_prompt();

        assert_eq!(browser.user_config.search_mode, SearchBackendMode::Simple);
        assert_eq!(
            browser.user_config.search_mode_prompt,
            SearchModePromptStatus::Completed
        );
        assert!(browser.search_mode_prompt.is_none());
        assert!(browser.startup_index_setup.is_none());
    }

    #[test]
    fn selecting_indexed_from_prompt_opens_startup_index_setup() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        browser.search_mode_prompt = Some(SearchModePromptState {
            selected_mode: Some(SearchBackendMode::Indexed),
        });
        browser.sidebar_locations = vec![SidebarLocation {
            label: "Documents".to_owned(),
            path: PathBuf::from("/home/user/Documents"),
            kind: SidebarLocationKind::Documents,
        }];

        let _task = browser.accept_search_mode_prompt();

        assert_eq!(browser.user_config.search_mode, SearchBackendMode::Indexed);
        assert_eq!(
            browser.user_config.search_mode_prompt,
            SearchModePromptStatus::Pending
        );
        assert!(browser.search_mode_prompt.is_none());
        assert!(browser.search_index.daemon_status_loading);
        assert!(browser.startup_index_setup.is_none());
    }

    #[test]
    fn prompt_next_without_selection_does_not_commit_mode() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        browser.search_mode_prompt = Some(SearchModePromptState::default());

        let _task = browser.accept_search_mode_prompt();

        assert_eq!(
            browser.user_config.search_mode_prompt,
            SearchModePromptStatus::Pending
        );
        assert!(browser.search_mode_prompt.is_some());
        assert!(browser.startup_index_setup.is_none());
    }

    #[test]
    fn switching_to_simple_clears_index_runtime_activity() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        browser.user_config.search_mode = SearchBackendMode::Indexed;
        browser
            .search_index
            .indexing_roots
            .insert(PathBuf::from("/home/user"));
        browser
            .search_index
            .status_loading_roots
            .insert(PathBuf::from("/home/user"), 1);
        browser.search = Some(SearchState {
            scope: SearchScope::CurrentDirectory,
            mode: SearchMode::Contents,
            session_backend_mode: SearchBackendMode::Indexed,
            root: PathBuf::from("/home/user"),
            query: "needle".to_owned(),
            request_generation: 1,
            search_cancel: None,
            matches: Vec::new(),
            selected_match: None,
            is_loading: false,
            is_indexing: true,
            skipped_count: 0,
            error: None,
            index_error: Some("previous index error".to_owned()),
            indexed_fallback_session: false,
            show_index_ready_reopen_hint: true,
        });

        let _task = browser.select_search_backend_mode(SearchBackendMode::Simple);

        assert_eq!(browser.user_config.search_mode, SearchBackendMode::Simple);
        assert!(browser.search_index.profile_roots.is_empty());
        assert!(browser.search_index.statuses.is_empty());
        assert!(browser.search_index.indexing_roots.is_empty());
        assert!(browser.search_index.status_loading_roots.is_empty());
        let search = browser.search.as_ref().expect("search remains open");
        assert_eq!(search.mode, SearchMode::Files);
        assert_eq!(search.session_backend_mode, SearchBackendMode::Simple);
        assert!(!search.indexed_fallback_session);
        assert!(!search.show_index_ready_reopen_hint);
        assert!(!search.is_indexing);
        assert!(search.index_error.is_none());
    }
}
