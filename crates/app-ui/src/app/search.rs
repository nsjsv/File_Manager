use std::path::{Path, PathBuf};
use std::time::Duration;

use file_core::{file_search_index_exists, FileKind, FileSearchIndexOutcome, FileSearchOutcome};
use iced::widget::{scrollable, text_input};
use iced::Command;

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

impl FileBrowser {
    pub(super) fn open_search(&mut self) -> Command<Message> {
        let root = self.search_root_for_scope(SearchScope::CurrentDirectory);
        let index_root = root.clone();
        self.context_menu = None;
        self.is_column_view_settings_open = false;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        let rename_command = self.commit_rename_if_active();
        self.file_drag = None;
        self.selection_marquee = None;
        self.search = Some(SearchState {
            scope: SearchScope::CurrentDirectory,
            root,
            query: String::new(),
            matches: Vec::new(),
            selected_match: None,
            is_loading: false,
            is_indexing: false,
            skipped_count: 0,
            error: None,
            index_error: None,
        });

        Command::batch([
            rename_command,
            self.ensure_search_window(),
            self.ensure_search_index(index_root),
            focus_search_input_command(),
            delayed_search_focus_commands(),
        ])
    }

    pub(super) fn update_search_query(&mut self, query: String) -> Command<Message> {
        let Some(search) = &mut self.search else {
            return Command::none();
        };

        search.query = query;
        search.matches.clear();
        search.selected_match = None;
        search.skipped_count = 0;
        search.error = None;
        self.load_search_matches()
    }

    pub(super) fn toggle_search_scope(&mut self) -> Command<Message> {
        let Some(next_scope) = self.search.as_ref().map(|search| match search.scope {
            SearchScope::CurrentDirectory => SearchScope::HomeDirectory,
            SearchScope::HomeDirectory => SearchScope::CurrentDirectory,
        }) else {
            return Command::none();
        };
        let root = self.search_root_for_scope(next_scope);
        let Some(search) = &mut self.search else {
            return Command::none();
        };

        search.scope = next_scope;
        search.root = root.clone();
        search.matches.clear();
        search.selected_match = None;
        search.skipped_count = 0;
        search.error = None;
        search.index_error = None;
        search.is_indexing = false;
        Command::batch([
            self.ensure_search_index(root),
            self.load_search_matches(),
            text_input::focus(search_input_id()),
        ])
    }

    pub(super) fn accept_search_matches(
        &mut self,
        request: SearchRequest,
        search: Result<FileSearchOutcome, String>,
    ) -> Command<Message> {
        let Some(state) = &mut self.search else {
            return Command::none();
        };
        if state.request() != request {
            return Command::none();
        }

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
        Command::none()
    }

    pub(super) fn accept_search_index(
        &mut self,
        root: PathBuf,
        outcome: Result<FileSearchIndexOutcome, String>,
    ) -> Command<Message> {
        self.search_index.indexing_roots.remove(&root);
        match outcome {
            Ok(outcome) => {
                self.search_index.errors.remove(&root);
                if self.search.as_ref().map(|search| &search.root) == Some(&root) {
                    if let Some(search) = &mut self.search {
                        search.is_indexing = false;
                        search.index_error = None;
                        if search.matches.is_empty() {
                            search.skipped_count = outcome.skipped.len();
                        }
                    }
                    return self.load_search_matches();
                }
            }
            Err(error) => {
                let message = format!("Failed to build search index: {error}");
                self.search_index
                    .errors
                    .insert(root.clone(), message.clone());
                if self.search.as_ref().map(|search| &search.root) == Some(&root) {
                    if let Some(search) = &mut self.search {
                        search.is_loading = false;
                        search.is_indexing = false;
                        search.index_error = Some(message);
                    }
                }
            }
        }

        Command::none()
    }

    pub(super) fn move_search_selection(
        &mut self,
        direction: PathSuggestionDirection,
    ) -> Command<Message> {
        let Some(search) = &mut self.search else {
            return Command::none();
        };
        if search.matches.is_empty() {
            search.selected_match = None;
            return Command::none();
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

    pub(super) fn activate_selected_search_match(&mut self) -> Command<Message> {
        let Some(path) = self.search.as_ref().and_then(|search| {
            search
                .selected_match
                .and_then(|index| search.matches.get(index))
                .map(|search_match| search_match.path.clone())
        }) else {
            return Command::none();
        };
        self.activate_search_match(path)
    }

    pub(super) fn activate_search_match(&mut self, path: PathBuf) -> Command<Message> {
        let Some(search_match) = self.search.as_ref().and_then(|search| {
            search
                .matches
                .iter()
                .find(|search_match| search_match.path == path)
                .cloned()
        }) else {
            return Command::none();
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
        Command::batch([close_command, activation_command])
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

    fn load_search_matches(&mut self) -> Command<Message> {
        let Some(request) = self.search.as_ref().map(SearchState::request) else {
            return Command::none();
        };
        if request.query.trim().is_empty() {
            let Some(search) = &mut self.search else {
                return Command::none();
            };
            search.is_loading = false;
            search.matches.clear();
            search.selected_match = None;
            search.skipped_count = 0;
            search.error = None;
            return Command::none();
        }

        let root = request.root.clone();
        let index_dir = self.search_index_dir_for_root(&root);
        let is_indexing = self.search_index.indexing_roots.contains(&root);
        let index_error = self.search_index.errors.get(&root).cloned();
        if let Some(search) = &mut self.search {
            search.is_indexing = is_indexing;
            search.index_error = index_error;
        }

        if !file_search_index_exists(&index_dir) {
            let index_command = self.ensure_search_index(root);
            if let Some(search) = &mut self.search {
                search.is_loading = true;
                search.error = None;
            }
            return Command::batch([
                index_command,
                search_tree_command(request, self.options.clone()),
            ]);
        }

        if let Some(search) = &mut self.search {
            search.is_loading = true;
            search.error = None;
        }
        search_command(request, self.options.clone(), index_dir)
    }

    fn search_root_for_scope(&self, scope: SearchScope) -> PathBuf {
        match scope {
            SearchScope::CurrentDirectory => self.current_dir.clone(),
            SearchScope::HomeDirectory => dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
        }
    }

    fn ensure_search_index(&mut self, root: PathBuf) -> Command<Message> {
        if self.search_index.indexing_roots.contains(&root) {
            self.sync_active_search_index_status();
            return Command::none();
        }

        let index_dir = self.search_index_dir_for_root(&root);
        if file_search_index_exists(&index_dir) {
            self.sync_active_search_index_status();
            return Command::none();
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
        let is_indexing = self.search_index.indexing_roots.contains(&root);
        let index_error = self.search_index.errors.get(&root).cloned();
        if let Some(search) = &mut self.search {
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

fn scroll_selected_search_match(search: &SearchState) -> Command<Message> {
    let Some(index) = search.selected_match else {
        return Command::none();
    };

    let y = selected_search_match_offset(index, search.matches.len());
    scrollable::scroll_to(
        search_results_id(),
        scrollable::AbsoluteOffset { x: 0.0, y },
    )
}

pub(super) fn focus_search_input_command() -> Command<Message> {
    text_input::focus(search_input_id())
}

fn delayed_search_focus_commands() -> Command<Message> {
    Command::batch(SEARCH_FOCUS_RETRY_DELAYS.map(|delay| {
        Command::perform(
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
}
