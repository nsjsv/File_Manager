use std::path::{Path, PathBuf};
use std::time::Duration;

use file_core::FileKind;
use file_index::{FileSearchIndexMode, FileSearchIndexOutcome, FileSearchOutcome};
use iced::widget::scrollable;
use iced::Task;
use tokio_util::sync::CancellationToken;

use super::FileBrowser;
use crate::commands::{search_command, search_index_command, search_tree_command};
use crate::config::SearchBackendMode;
use crate::model::{
    Message, NavigationMode, PathSuggestionDirection, SearchMode, SearchRequest, SearchScope,
    SearchState, SidebarLocationKind,
};
use crate::view::{
    search_input_id, search_results_id, SEARCH_RESULTS_HEIGHT, SEARCH_RESULTS_PADDING,
    SEARCH_RESULT_ROW_HEIGHT, SEARCH_RESULT_ROW_SPACING,
};

const SEARCH_FOCUS_RETRY_DELAYS: [Duration; 2] =
    [Duration::from_millis(16), Duration::from_millis(75)];
const SEARCH_INPUT_STABILIZATION_DELAY: Duration = Duration::from_millis(150);

impl FileBrowser {
    pub(super) fn open_search(&mut self) -> Task<Message> {
        let root = self.search_root_for_scope(SearchScope::CurrentDirectory);
        self.context_menu = None;
        self.open_with = None;
        self.archive_creation = None;
        self.archive_extraction = None;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        let rename_command = self.commit_rename_if_active();
        self.file_drag = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.selection_marquee = None;
        self.search = Some(SearchState {
            scope: SearchScope::CurrentDirectory,
            mode: SearchMode::Files,
            root: root.clone(),
            query: String::new(),
            request_generation: 0,
            search_cancel: None,
            matches: Vec::new(),
            selected_match: None,
            is_loading: false,
            is_indexing: false,
            skipped_count: 0,
            error: None,
            index_error: None,
        });

        let index_command = if self.user_config.search_mode == SearchBackendMode::Indexed {
            self.ensure_search_index(root)
        } else {
            Task::none()
        };

        Task::batch([
            rename_command,
            self.ensure_search_window(),
            index_command,
            focus_search_input_command(),
            delayed_search_focus_commands(),
        ])
    }

    pub(super) fn update_search_query(&mut self, query: String) -> Task<Message> {
        let Some(search) = &mut self.search else {
            return Task::none();
        };

        search.request_generation = search.request_generation.wrapping_add(1);
        if let Some(cancel) = search.search_cancel.take() {
            cancel.cancel();
        }
        search.query = query;
        search.matches.clear();
        search.selected_match = None;
        search.skipped_count = 0;
        search.error = None;
        let request = search.request();
        if request.query.trim().is_empty() {
            search.is_loading = false;
            return Task::none();
        }

        search.is_loading = true;
        Task::batch([
            search_input_stabilization_command(request),
            self.request_browser_session_save(),
        ])
    }

    pub(super) fn load_stable_search_matches(&mut self, request: SearchRequest) -> Task<Message> {
        if !self.active_search_matches_request(&request) {
            return Task::none();
        }
        self.load_search_matches()
    }

    pub(super) fn toggle_search_scope(&mut self) -> Task<Message> {
        let Some(next_scope) = self.search.as_ref().map(|search| match search.scope {
            SearchScope::CurrentDirectory => SearchScope::HomeDirectory,
            SearchScope::HomeDirectory => SearchScope::CurrentDirectory,
        }) else {
            return Task::none();
        };
        let root = self.search_root_for_scope(next_scope);
        let Some(search) = &mut self.search else {
            return Task::none();
        };

        search.scope = next_scope;
        search.root = root.clone();
        search.request_generation = search.request_generation.wrapping_add(1);
        if let Some(cancel) = search.search_cancel.take() {
            cancel.cancel();
        }
        search.matches.clear();
        search.selected_match = None;
        search.skipped_count = 0;
        search.error = None;
        search.index_error = None;
        search.is_indexing = false;
        let index_command = if self.user_config.search_mode == SearchBackendMode::Indexed {
            self.ensure_search_index(root)
        } else {
            Task::none()
        };
        Task::batch([
            index_command,
            self.load_search_matches(),
            iced::widget::operation::focus(search_input_id()),
            self.request_browser_session_save(),
        ])
    }

    pub(super) fn select_search_mode(&mut self, mode: SearchMode) -> Task<Message> {
        if self.user_config.search_mode == SearchBackendMode::Simple && mode != SearchMode::Files {
            return Task::none();
        }
        let Some(search) = &mut self.search else {
            return Task::none();
        };
        if search.mode == mode {
            return Task::none();
        }

        search.mode = mode;
        search.request_generation = search.request_generation.wrapping_add(1);
        if let Some(cancel) = search.search_cancel.take() {
            cancel.cancel();
        }
        search.matches.clear();
        search.selected_match = None;
        search.skipped_count = 0;
        search.error = None;
        search.index_error = None;
        Task::batch([
            self.load_search_matches(),
            self.request_browser_session_save(),
        ])
    }

    pub(super) fn accept_search_matches(
        &mut self,
        request: SearchRequest,
        search: Result<FileSearchOutcome, String>,
    ) -> Task<Message> {
        let Some(state) = self.active_search_mut_for_request(&request) else {
            return Task::none();
        };

        state.is_loading = false;
        if let Some(cancel) = state.search_cancel.take() {
            cancel.cancel();
        }
        match search {
            Ok(search) => {
                state.matches = search.matches;
                state.skipped_count = search.skipped.len();
                state.error = None;
                normalize_search_selection(state);
                return scroll_selected_search_match(state);
            }
            Err(error) => {
                state.matches.clear();
                state.selected_match = None;
                state.skipped_count = 0;
                state.error = Some(error);
            }
        }
        Task::none()
    }

    pub(super) fn accept_search_index(
        &mut self,
        root: PathBuf,
        outcome: Result<FileSearchIndexOutcome, String>,
    ) -> Task<Message> {
        self.search_index.indexing_roots.remove(&root);
        match outcome {
            Ok(outcome) => {
                self.search_index.root_errors.remove(&root);
                let mut reload_search = false;
                if let Some(search) = self.active_search_mut_for_root(&root) {
                    search.is_indexing = false;
                    search.index_error = None;
                    if search.matches.is_empty() {
                        search.skipped_count = outcome.skipped.len();
                    }
                    reload_search = true;
                }
                let status_command = self.force_refresh_search_index_status_for_root(root.clone());
                return if reload_search {
                    Task::batch([self.load_search_matches(), status_command])
                } else {
                    status_command
                };
            }
            Err(error) => {
                let message = format!("Failed to build search index: {error}");
                self.search_index
                    .root_errors
                    .insert(root.clone(), message.clone());
                if let Some(search) = self.active_search_mut_for_root(&root) {
                    search.is_loading = false;
                    search.is_indexing = false;
                    search.index_error = Some(message);
                }
            }
        }

        Task::none()
    }

    pub(super) fn move_search_selection(
        &mut self,
        direction: PathSuggestionDirection,
    ) -> Task<Message> {
        let Some(search) = &mut self.search else {
            return Task::none();
        };
        if search.matches.is_empty() {
            search.selected_match = None;
            return Task::none();
        }

        let last_index = search.matches.len() - 1;
        let Some(current) = search.selected_match else {
            search.selected_match = Some(match direction {
                PathSuggestionDirection::Next => 0,
                PathSuggestionDirection::Previous => last_index,
            });
            return scroll_selected_search_match(search);
        };

        search.selected_match = Some(match direction {
            PathSuggestionDirection::Next => {
                if current >= last_index {
                    0
                } else {
                    current + 1
                }
            }
            PathSuggestionDirection::Previous => {
                if current == 0 {
                    last_index
                } else {
                    current - 1
                }
            }
        });
        scroll_selected_search_match(search)
    }

    pub(super) fn activate_selected_search_match(&mut self) -> Task<Message> {
        let Some(path) = self.search.as_ref().and_then(|search| {
            search
                .selected_match
                .and_then(|index| search.matches.get(index))
                .map(|search_match| search_match.path.clone())
        }) else {
            return Task::none();
        };
        self.activate_search_match(path)
    }

    pub(super) fn activate_search_match(&mut self, path: PathBuf) -> Task<Message> {
        let Some(search_match) = self.search.as_ref().and_then(|search| {
            search
                .matches
                .iter()
                .find(|search_match| search_match.path == path)
                .cloned()
        }) else {
            return Task::none();
        };

        let close_command = self.close_search_window();
        self.context_menu = None;
        let activation_command = if search_match.kind == FileKind::Directory {
            self.navigate_to(search_match.path, NavigationMode::RecordHistory)
        } else {
            let parent = search_match
                .path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.current_dir.clone());
            self.pending_search_reveal = Some(search_match.path);
            self.navigate_to(parent, NavigationMode::RecordHistory)
        };
        Task::batch([close_command, activation_command])
    }

    pub(super) fn reveal_pending_search_match(&mut self) {
        let Some(path) = self.pending_search_reveal.clone() else {
            return;
        };
        let parent_matches = path.parent() == Some(self.current_dir.as_path());
        if parent_matches && self.entries.iter().any(|entry| entry.path == path) {
            self.select_path(path);
        }
        self.pending_search_reveal = None;
    }

    pub(super) fn load_search_matches(&mut self) -> Task<Message> {
        let Some(request) = self.active_search_request() else {
            return Task::none();
        };
        if request.query.trim().is_empty() {
            self.clear_active_search_results();
            return Task::none();
        }
        if self.user_config.search_mode == SearchBackendMode::Simple {
            self.mark_active_search_loading();
            self.clear_active_search_index_status_for_root(&request.root);
            let cancellation = self.replace_active_search_cancel_token();
            return search_tree_command(
                request,
                self.options.clone(),
                self.user_config.search_index_exclude_patterns.clone(),
                self.user_config.search_index_directory_error_policy,
                cancellation,
            );
        }
        let root = request.root.clone();
        self.sync_active_search_index_status_for_root(&root);
        let status = self.search_index.statuses.get(&root).cloned();

        if status
            .as_ref()
            .is_none_or(|status| !status.exists || status.stale)
        {
            let index_command = self.ensure_search_index(root);
            self.mark_active_search_loading();
            self.clear_active_search_cancel_token();
            return index_command;
        }

        self.mark_active_search_loading();
        self.clear_active_search_cancel_token();
        search_command(
            request,
            self.options.clone(),
            self.user_config.clone(),
            self.search_index.profile_id.clone(),
        )
    }

    fn search_root_for_scope(&self, scope: SearchScope) -> PathBuf {
        match scope {
            SearchScope::CurrentDirectory => self
                .cursor_search_directory
                .clone()
                .unwrap_or_else(|| self.current_dir.clone()),
            SearchScope::HomeDirectory => self.home_search_root(),
        }
    }

    fn home_search_root(&self) -> PathBuf {
        self.sidebar_locations
            .iter()
            .find(|location| location.kind == SidebarLocationKind::Home)
            .map(|location| location.path.clone())
            .unwrap_or_else(|| self.current_dir.clone())
    }

    pub(super) fn ensure_search_index(&mut self, root: PathBuf) -> Task<Message> {
        if !self.search_index_root_is_allowed(&root) {
            self.search_index.root_errors.insert(
                root.clone(),
                "Only paths under your home directory can be indexed.".to_owned(),
            );
            self.sync_active_search_index_status();
            return Task::none();
        }
        if self.search_index.indexing_roots.contains(&root) {
            self.sync_active_search_index_status();
            return Task::none();
        }

        if self
            .search_index
            .statuses
            .get(&root)
            .is_some_and(|status| status.exists && !status.stale)
        {
            self.sync_active_search_index_status();
            return Task::none();
        }

        self.search_index.indexing_roots.insert(root.clone());
        self.search_index.root_errors.remove(&root);
        self.sync_active_search_index_status();
        search_index_command(
            root,
            self.user_config.clone(),
            self.search_index.profile_id.clone(),
            FileSearchIndexMode::FullRebuild,
        )
    }

    fn sync_active_search_index_status(&mut self) {
        let Some(root) = self.search.as_ref().map(|search| search.root.clone()) else {
            return;
        };
        self.sync_active_search_index_status_for_root(&root);
    }

    fn active_search_request(&self) -> Option<SearchRequest> {
        self.search.as_ref().map(SearchState::request)
    }

    fn active_search_mut_for_request(
        &mut self,
        request: &SearchRequest,
    ) -> Option<&mut SearchState> {
        self.search
            .as_mut()
            .filter(|search| search_request_matches_active_state(search, request))
    }

    fn active_search_matches_request(&self, request: &SearchRequest) -> bool {
        matches!(
            self.search.as_ref(),
            Some(search) if search_request_matches_active_state(search, request)
        )
    }

    pub(super) fn active_search_mut_for_root(&mut self, root: &Path) -> Option<&mut SearchState> {
        self.search
            .as_mut()
            .filter(|search| search.root.as_path() == root)
    }

    fn clear_active_search_results(&mut self) {
        if let Some(search) = &mut self.search {
            search.is_loading = false;
            search.matches.clear();
            search.selected_match = None;
            search.skipped_count = 0;
            search.error = None;
        }
    }

    fn mark_active_search_loading(&mut self) {
        if let Some(search) = &mut self.search {
            search.is_loading = true;
            search.error = None;
        }
    }

    fn replace_active_search_cancel_token(&mut self) -> CancellationToken {
        let cancellation = CancellationToken::new();
        if let Some(search) = &mut self.search {
            if let Some(previous) = search.search_cancel.replace(cancellation.clone()) {
                previous.cancel();
            }
        }
        cancellation
    }

    fn clear_active_search_cancel_token(&mut self) {
        if let Some(search) = &mut self.search {
            if let Some(cancel) = search.search_cancel.take() {
                cancel.cancel();
            }
        }
    }

    fn sync_active_search_index_status_for_root(&mut self, root: &Path) {
        let is_indexing = self.search_index.indexing_roots.contains(root);
        let index_error = self.search_index.root_errors.get(root).cloned();
        if let Some(search) = self.active_search_mut_for_root(root) {
            search.is_indexing = is_indexing;
            search.index_error = index_error;
        }
    }

    fn clear_active_search_index_status_for_root(&mut self, root: &Path) {
        if let Some(search) = self.active_search_mut_for_root(root) {
            search.is_indexing = false;
            search.index_error = None;
        }
    }
}

fn normalize_search_selection(search: &mut SearchState) {
    if search.matches.is_empty() {
        search.selected_match = None;
        return;
    }

    search.selected_match = search
        .selected_match
        .filter(|index| *index < search.matches.len())
        .or(Some(0));
}

fn scroll_selected_search_match(search: &SearchState) -> Task<Message> {
    let Some(index) = search.selected_match else {
        return Task::none();
    };

    let y = selected_search_match_offset(index, search.matches.len());
    iced::widget::operation::scroll_to(
        search_results_id(),
        scrollable::AbsoluteOffset { x: 0.0, y },
    )
}

fn search_input_stabilization_command(request: SearchRequest) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(SEARCH_INPUT_STABILIZATION_DELAY).await;
            request
        },
        Message::SearchInputStabilized,
    )
}

fn search_request_matches_active_state(search: &SearchState, request: &SearchRequest) -> bool {
    search.request() == *request
}

pub(super) fn focus_search_input_command() -> Task<Message> {
    iced::widget::operation::focus(search_input_id())
}

fn delayed_search_focus_commands() -> Task<Message> {
    Task::batch(SEARCH_FOCUS_RETRY_DELAYS.map(|delay| {
        Task::perform(
            async move {
                tokio::time::sleep(delay).await;
            },
            |_| Message::SearchFocusRequested,
        )
    }))
}

fn selected_search_match_offset(index: usize, match_count: usize) -> f32 {
    if match_count == 0 {
        return 0.0;
    }

    let row_pitch = SEARCH_RESULT_ROW_HEIGHT + SEARCH_RESULT_ROW_SPACING;
    let selected_center = SEARCH_RESULTS_PADDING
        + index.min(match_count - 1) as f32 * row_pitch
        + SEARCH_RESULT_ROW_HEIGHT / 2.0;
    let content_height = search_results_content_height(match_count);
    let max_offset = (content_height - SEARCH_RESULTS_HEIGHT).max(0.0);
    (selected_center - SEARCH_RESULTS_HEIGHT / 2.0).clamp(0.0, max_offset)
}

fn search_results_content_height(match_count: usize) -> f32 {
    if match_count == 0 {
        return 0.0;
    }

    SEARCH_RESULTS_PADDING * 2.0
        + match_count as f32 * SEARCH_RESULT_ROW_HEIGHT
        + match_count.saturating_sub(1) as f32 * SEARCH_RESULT_ROW_SPACING
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
