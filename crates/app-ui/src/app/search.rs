use std::path::{Path, PathBuf};
use std::time::Duration;

use file_core::{file_search_index_exists, FileKind, FileSearchIndexOutcome, FileSearchOutcome};
use iced::widget::scrollable;
use iced::Task;

use super::FileBrowser;
use crate::commands::{search_command, search_index_command, search_tree_command};
use crate::config::search_index_dir_for_root;
use crate::model::{
    Message, NavigationMode, PathSuggestionDirection, SearchRequest, SearchScope, SearchState,
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
        let index_root = root.clone();
        self.context_menu = None;
        self.is_column_view_settings_open = false;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        let rename_command = self.commit_rename_if_active();
        self.file_drag = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.selection_marquee = None;
        self.search = Some(SearchState {
            scope: SearchScope::CurrentDirectory,
            root,
            query: String::new(),
            request_generation: 0,
            matches: Vec::new(),
            selected_match: None,
            is_loading: false,
            is_indexing: false,
            skipped_count: 0,
            error: None,
            index_error: None,
        });

        Task::batch([
            rename_command,
            self.ensure_search_window(),
            self.ensure_search_index(index_root),
            focus_search_input_command(),
            delayed_search_focus_commands(),
        ])
    }

    pub(super) fn update_search_query(&mut self, query: String) -> Task<Message> {
        let Some(search) = &mut self.search else {
            return Task::none();
        };

        search.request_generation = search.request_generation.wrapping_add(1);
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
        search_input_stabilization_command(request)
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
        search.matches.clear();
        search.selected_match = None;
        search.skipped_count = 0;
        search.error = None;
        search.index_error = None;
        search.is_indexing = false;
        Task::batch([
            self.ensure_search_index(root),
            self.load_search_matches(),
            iced::widget::operation::focus(search_input_id()),
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
                self.search_index.errors.remove(&root);
                if let Some(search) = self.active_search_mut_for_root(&root) {
                    search.is_indexing = false;
                    search.index_error = None;
                    if search.matches.is_empty() {
                        search.skipped_count = outcome.skipped.len();
                    }
                    return self.load_search_matches();
                }
            }
            Err(error) => {
                let message = format!("Failed to build search index: {error}");
                self.search_index
                    .errors
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

    fn load_search_matches(&mut self) -> Task<Message> {
        let Some(request) = self.active_search_request() else {
            return Task::none();
        };
        if request.query.trim().is_empty() {
            self.clear_active_search_results();
            return Task::none();
        }

        let root = request.root.clone();
        let index_dir = self.search_index_dir_for_root(&root);
        self.sync_active_search_index_status_for_root(&root);

        if !file_search_index_exists(&index_dir) {
            let index_command = self.ensure_search_index(root);
            self.mark_active_search_loading();
            return Task::batch([
                index_command,
                search_tree_command(request, self.options.clone()),
            ]);
        }

        self.mark_active_search_loading();
        search_command(request, self.options.clone(), index_dir)
    }

    fn search_root_for_scope(&self, scope: SearchScope) -> PathBuf {
        match scope {
            SearchScope::CurrentDirectory => self
                .cursor_search_directory
                .clone()
                .unwrap_or_else(|| self.current_dir.clone()),
            SearchScope::HomeDirectory => dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
        }
    }

    fn ensure_search_index(&mut self, root: PathBuf) -> Task<Message> {
        if self.search_index.indexing_roots.contains(&root) {
            self.sync_active_search_index_status();
            return Task::none();
        }

        let index_dir = self.search_index_dir_for_root(&root);
        if file_search_index_exists(&index_dir) {
            self.sync_active_search_index_status();
            return Task::none();
        }

        self.search_index.indexing_roots.insert(root.clone());
        self.search_index.errors.remove(&root);
        self.sync_active_search_index_status();
        search_index_command(root, index_dir, self.options.clone())
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

    fn active_search_mut_for_root(&mut self, root: &Path) -> Option<&mut SearchState> {
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

    fn sync_active_search_index_status_for_root(&mut self, root: &Path) {
        let is_indexing = self.search_index.indexing_roots.contains(root);
        let index_error = self.search_index.errors.get(root).cloned();
        if let Some(search) = self.active_search_mut_for_root(root) {
            search.is_indexing = is_indexing;
            search.index_error = index_error;
        }
    }

    fn search_index_dir_for_root(&self, root: &Path) -> PathBuf {
        search_index_dir_for_root(&self.search_index.base_dir, root)
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
mod tests {
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
        let max_offset =
            (search_results_content_height(match_count) - SEARCH_RESULTS_HEIGHT).max(0.0);

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

    fn search_state_for_request_generation(request_generation: u64) -> SearchState {
        SearchState {
            scope: SearchScope::CurrentDirectory,
            root: PathBuf::from("/tmp"),
            query: "needle".to_owned(),
            request_generation,
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
            root: search.root.clone(),
            query: search.query.clone(),
            generation,
        }
    }

    fn search_match_at_path(path: &str) -> file_core::FileSearchMatch {
        let path = PathBuf::from(path);
        let name = path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("match"))
            .to_os_string();
        file_core::FileSearchMatch {
            relative_path: PathBuf::from(&name),
            path,
            name,
            kind: FileKind::File,
            rank_score: 1,
        }
    }
}
