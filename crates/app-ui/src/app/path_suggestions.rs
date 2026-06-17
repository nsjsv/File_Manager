use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::Task;

use super::paths::{completed_path_text, path_text};
use super::FileBrowser;
use crate::commands::path_suggestions_command;
use crate::model::{
    BrowserPaneId, Message, NavigationMode, PathSuggestionDirection, PathSuggestionRequest,
};
use crate::view::path_input_id;

const PATH_SUGGESTION_INPUT_STABILIZATION_DELAY: Duration = Duration::from_millis(120);

impl FileBrowser {
    pub(super) fn focus_active_path_input(&mut self) -> Task<Message> {
        if self.destructive_action_confirmation.is_some()
            || self.transfer_conflict.is_some()
            || self.archive_creation.is_some()
        {
            return Task::none();
        }

        self.context_menu = None;
        self.shortcut_capture = None;
        self.operation_queue.close_panel();
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        let input_id = path_input_id(self.active_pane_id());
        Task::batch([
            self.commit_rename_if_active(),
            iced::widget::operation::focus(input_id.clone()),
            iced::widget::operation::select_all(input_id),
        ])
    }

    pub(super) fn update_path_input(&mut self, value: String) -> Task<Message> {
        self.path_input = value;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        let request = self.next_path_suggestion_request();
        if request.input.trim().is_empty() {
            return Task::none();
        }
        path_suggestion_input_stabilization_command(self.active_pane_id(), request)
    }

    pub(super) fn load_stable_path_suggestions(
        &mut self,
        pane_id: BrowserPaneId,
        request: PathSuggestionRequest,
    ) -> Task<Message> {
        if pane_id != self.active_pane_id() {
            let Some(pane) = self.pane_by_id(pane_id) else {
                return Task::none();
            };
            if !path_suggestion_request_matches_state(
                &request,
                &pane.path_input,
                &pane.current_dir,
                pane.path_suggestion_generation,
            ) {
                return Task::none();
            }
            return path_suggestions_command(pane_id, request);
        }

        if !self.active_path_suggestion_request_matches(&request) {
            return Task::none();
        }
        path_suggestions_command(pane_id, request)
    }

    pub(super) fn accept_path_suggestions(
        &mut self,
        pane_id: BrowserPaneId,
        request: PathSuggestionRequest,
        suggestions: Vec<PathBuf>,
    ) -> Task<Message> {
        if pane_id != self.active_pane_id() {
            let Some(pane) = self.pane_by_id_mut(pane_id) else {
                return Task::none();
            };
            if path_suggestion_request_matches_state(
                &request,
                &pane.path_input,
                &pane.current_dir,
                pane.path_suggestion_generation,
            ) {
                pane.path_suggestions = suggestions;
                if pane.path_suggestions.is_empty() {
                    pane.path_suggestion_selection = None;
                } else {
                    pane.path_suggestion_selection = pane
                        .path_suggestion_selection
                        .filter(|index| *index < pane.path_suggestions.len());
                }
            }
            return Task::none();
        }

        if self.active_path_suggestion_request_matches(&request) {
            self.path_suggestions = suggestions;
            self.normalize_path_suggestion_selection();
        }
        Task::none()
    }

    pub(super) fn move_search_or_path_suggestion_selection(
        &mut self,
        direction: PathSuggestionDirection,
    ) -> Task<Message> {
        if self.search.is_some() {
            return self.move_search_selection(direction);
        }
        self.move_path_suggestion_selection(direction);
        Task::none()
    }

    pub(super) fn complete_search_scope_or_path_suggestion(
        &mut self,
        direction: PathSuggestionDirection,
    ) -> Task<Message> {
        if self.search.is_some() {
            self.toggle_search_scope()
        } else {
            self.complete_path_suggestion(direction)
        }
    }

    pub(super) fn submit_path_input(&mut self) -> Task<Message> {
        let Some(typed_path) = self.path_from_input() else {
            self.path_input = path_text(&self.current_dir);
            self.path_suggestions.clear();
            self.path_suggestion_selection = None;
            return Task::none();
        };

        let selected_suggestion = self
            .path_suggestion_selection
            .and_then(|index| self.path_suggestions.get(index))
            .cloned();

        let path = selected_suggestion.unwrap_or(typed_path);

        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        Task::batch([
            self.navigate_to(path, NavigationMode::RecordHistory),
            iced::widget::operation::move_cursor_to_end(path_input_id(self.active_pane_id())),
        ])
    }

    pub(super) fn normalize_path_suggestion_selection(&mut self) {
        if self.path_suggestions.is_empty() {
            self.path_suggestion_selection = None;
            return;
        }

        self.path_suggestion_selection = self
            .path_suggestion_selection
            .filter(|index| *index < self.path_suggestions.len());
    }

    pub(super) fn move_path_suggestion_selection(&mut self, direction: PathSuggestionDirection) {
        if self.path_suggestions.is_empty() {
            self.path_suggestion_selection = None;
            return;
        }

        let last_index = self.path_suggestions.len() - 1;
        let Some(current) = self.path_suggestion_selection else {
            self.path_suggestion_selection = Some(match direction {
                PathSuggestionDirection::Next => 0,
                PathSuggestionDirection::Previous => last_index,
            });
            return;
        };
        self.path_suggestion_selection = Some(match direction {
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
    }

    pub(super) fn complete_path_suggestion(
        &mut self,
        direction: PathSuggestionDirection,
    ) -> Task<Message> {
        if self.path_suggestions.is_empty() {
            return Task::none();
        }

        if self.path_suggestion_selection.is_none() {
            self.path_suggestion_selection = Some(0);
        } else if direction == PathSuggestionDirection::Previous {
            self.move_path_suggestion_selection(direction);
        }

        let Some(path) = self
            .path_suggestion_selection
            .and_then(|index| self.path_suggestions.get(index))
            .cloned()
        else {
            return Task::none();
        };

        self.path_input = completed_path_text(&path);
        let request = self.next_path_suggestion_request();
        Task::batch([
            path_suggestions_command(self.active_pane_id(), request),
            iced::widget::operation::move_cursor_to_end(path_input_id(self.active_pane_id())),
        ])
    }

    fn path_from_input(&self) -> Option<PathBuf> {
        let trimmed = self.path_input.trim();
        if trimmed.is_empty() {
            return None;
        }

        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            Some(path)
        } else {
            Some(self.current_dir.join(path))
        }
    }

    fn next_path_suggestion_request(&mut self) -> PathSuggestionRequest {
        self.path_suggestion_generation = self.path_suggestion_generation.wrapping_add(1);
        PathSuggestionRequest {
            input: self.path_input.clone(),
            current_dir: self.current_dir.clone(),
            generation: self.path_suggestion_generation,
        }
    }

    fn active_path_suggestion_request_matches(&self, request: &PathSuggestionRequest) -> bool {
        path_suggestion_request_matches_state(
            request,
            &self.path_input,
            &self.current_dir,
            self.path_suggestion_generation,
        )
    }
}

fn path_suggestion_input_stabilization_command(
    pane_id: BrowserPaneId,
    request: PathSuggestionRequest,
) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(PATH_SUGGESTION_INPUT_STABILIZATION_DELAY).await;
            request
        },
        move |request| Message::PathInputStabilized(pane_id, request),
    )
}

fn path_suggestion_request_matches_state(
    request: &PathSuggestionRequest,
    input: &str,
    current_dir: &Path,
    generation: u64,
) -> bool {
    request.input == input
        && request.current_dir.as_path() == current_dir
        && request.generation == generation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_suggestion_request_generation_rejects_repeated_stale_input() {
        let current_dir = PathBuf::from("/tmp");
        let request = PathSuggestionRequest {
            input: "docs".to_owned(),
            current_dir: current_dir.clone(),
            generation: 4,
        };

        assert!(!path_suggestion_request_matches_state(
            &request,
            "docs",
            &current_dir,
            5
        ));
        assert!(path_suggestion_request_matches_state(
            &request,
            "docs",
            &current_dir,
            4
        ));
    }

    #[test]
    fn stale_path_input_stabilization_keeps_current_suggestions() {
        let current_suggestion = PathBuf::from("/tmp/current");
        let mut browser = browser_with_path_suggestion_state(
            "docs",
            2,
            vec![current_suggestion.clone()],
            Some(0),
        );
        let stale_request = path_suggestion_request("docs", &browser.current_dir, 1);

        assert!(!browser.active_path_suggestion_request_matches(&stale_request));
        let _command = browser.load_stable_path_suggestions(BrowserPaneId::PRIMARY, stale_request);

        assert_eq!(browser.path_suggestions, vec![current_suggestion]);
        assert_eq!(browser.path_suggestion_selection, Some(0));
        assert_eq!(browser.path_suggestion_generation, 2);
    }

    #[test]
    fn stale_path_suggestions_loaded_keeps_current_suggestions() {
        let current_suggestion = PathBuf::from("/tmp/current");
        let mut browser = browser_with_path_suggestion_state(
            "docs",
            2,
            vec![current_suggestion.clone()],
            Some(0),
        );
        let stale_request = path_suggestion_request("docs", &browser.current_dir, 1);

        let _command = browser.accept_path_suggestions(
            BrowserPaneId::PRIMARY,
            stale_request,
            vec![PathBuf::from("/tmp/stale")],
        );

        assert_eq!(browser.path_suggestions, vec![current_suggestion]);
        assert_eq!(browser.path_suggestion_selection, Some(0));
    }

    #[test]
    fn empty_path_input_clears_suggestions_and_advances_generation() {
        let mut browser = browser_with_path_suggestion_state(
            "docs",
            9,
            vec![PathBuf::from("/tmp/current")],
            Some(0),
        );

        let _command = browser.update_path_input("  ".to_owned());

        assert_eq!(browser.path_input, "  ");
        assert!(browser.path_suggestions.is_empty());
        assert_eq!(browser.path_suggestion_selection, None);
        assert_eq!(browser.path_suggestion_generation, 10);
    }

    fn browser_with_path_suggestion_state(
        input: &str,
        generation: u64,
        suggestions: Vec<PathBuf>,
        selection: Option<usize>,
    ) -> FileBrowser {
        let (mut browser, _) = FileBrowser::new(crate::config::default_user_config());
        browser.current_dir = PathBuf::from("/tmp");
        browser.path_input = input.to_owned();
        browser.path_suggestion_generation = generation;
        browser.path_suggestions = suggestions;
        browser.path_suggestion_selection = selection;
        browser
    }

    fn path_suggestion_request(
        input: &str,
        current_dir: &Path,
        generation: u64,
    ) -> PathSuggestionRequest {
        PathSuggestionRequest {
            input: input.to_owned(),
            current_dir: current_dir.to_path_buf(),
            generation,
        }
    }
}
