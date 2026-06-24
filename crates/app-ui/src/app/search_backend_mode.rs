use iced::Task;

use super::FileBrowser;
use crate::config::{SearchBackendMode, SearchModePromptStatus};
use crate::model::{Message, SearchModePromptState, SettingsCategory};

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
        self.startup_index_setup = None;
        self.search_index.pending_startup_index_builds.clear();
        self.user_config.search_mode = SearchBackendMode::Simple;
        self.user_config.search_mode_prompt = SearchModePromptStatus::Completed;
        self.search_index.service_generation = self.search_index.service_generation.wrapping_add(1);
        self.persist_user_config_command()
    }

    pub(super) fn select_indexed_search_from_prompt(&mut self) -> Task<Message> {
        self.search_mode_prompt = None;
        self.user_config.search_mode = SearchBackendMode::Indexed;
        self.search_index.service_generation = self.search_index.service_generation.wrapping_add(1);
        Task::batch([
            self.load_search_index_profile_for_mode(),
            self.refresh_startup_index_setup_choices(),
        ])
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
                self.startup_index_setup = None;
                self.clear_indexed_search_state();
                self.persist_user_config_command()
            }
            SearchBackendMode::Indexed => Task::batch([
                self.persist_user_config_command(),
                self.load_search_index_profile_for_mode(),
                self.refresh_search_index_statuses(),
            ]),
        }
    }

    pub(crate) fn search_backend_mode(&self) -> SearchBackendMode {
        self.user_config.search_mode
    }

    fn load_search_index_profile_for_mode(&mut self) -> Task<Message> {
        if self.selected_settings_category == SettingsCategory::SearchIndex
            || self.user_config.search_mode == SearchBackendMode::Indexed
        {
            self.load_search_index_profile_command()
        } else {
            Task::none()
        }
    }

    fn clear_indexed_search_state(&mut self) {
        self.search_index.maintenance_paused = false;
        self.search_index.status_loading_roots.clear();
        self.search_index.indexing_roots.clear();
        self.search_index.pending_startup_index_builds.clear();
        if let Some(search) = &mut self.search {
            if search.mode != crate::model::SearchMode::Files {
                search.mode = crate::model::SearchMode::Files;
                search.request_generation = search.request_generation.wrapping_add(1);
                search.matches.clear();
                search.selected_match = None;
                search.skipped_count = 0;
            }
            search.is_indexing = false;
            search.index_error = None;
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
        assert!(browser.startup_index_setup.is_some());
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
        browser.search_index.maintenance_paused = true;
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
        });

        let _task = browser.select_search_backend_mode(SearchBackendMode::Simple);

        assert_eq!(browser.user_config.search_mode, SearchBackendMode::Simple);
        assert!(!browser.search_index.maintenance_paused);
        assert!(browser.search_index.indexing_roots.is_empty());
        assert!(browser.search_index.status_loading_roots.is_empty());
        let search = browser.search.as_ref().expect("search remains open");
        assert_eq!(search.mode, SearchMode::Files);
        assert!(!search.is_indexing);
        assert!(search.index_error.is_none());
    }
}
