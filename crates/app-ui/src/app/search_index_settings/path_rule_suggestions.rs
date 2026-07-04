use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::Task;

use crate::app::FileBrowser;
use crate::commands::search_index_path_rule_suggestions_command;
use crate::model::{Message, PathSuggestionRequest, SearchIndexPathRuleKind};

use super::search_index_display_path;

const SEARCH_INDEX_PATH_RULE_SUGGESTION_DELAY: Duration = Duration::from_millis(120);

impl FileBrowser {
    pub(crate) fn update_search_index_path_rule_input(&mut self, input: String) -> Task<Message> {
        self.search_index.path_rule_input = input;
        self.search_index.path_rule_error = None;
        self.clear_search_index_path_rule_suggestions();

        let Some(request) = self.next_search_index_path_rule_suggestion_request() else {
            return Task::none();
        };

        search_index_path_rule_input_stabilization_command(request)
    }

    pub(crate) fn load_stable_search_index_path_rule_suggestions(
        &mut self,
        request: PathSuggestionRequest,
    ) -> Task<Message> {
        if !self.search_index_path_rule_suggestion_request_matches(&request) {
            return Task::none();
        }

        search_index_path_rule_suggestions_command(request)
    }

    pub(crate) fn accept_search_index_path_rule_suggestions(
        &mut self,
        request: PathSuggestionRequest,
        suggestions: Vec<PathBuf>,
    ) -> Task<Message> {
        if self.search_index_path_rule_suggestion_request_matches(&request) {
            self.search_index.path_rule_suggestions = suggestions;
        }

        Task::none()
    }

    pub(crate) fn select_search_index_path_rule_suggestion(
        &mut self,
        path: PathBuf,
    ) -> Task<Message> {
        let home = self.search_index_home_directory();
        self.search_index.path_rule_input = search_index_display_path(&path, &home);
        self.clear_search_index_path_rule_suggestions();
        Task::none()
    }

    pub(crate) fn clear_search_index_path_rule_suggestions(&mut self) {
        self.search_index.path_rule_suggestions.clear();
    }

    fn next_search_index_path_rule_suggestion_request(&mut self) -> Option<PathSuggestionRequest> {
        self.search_index.path_rule_suggestion_generation = self
            .search_index
            .path_rule_suggestion_generation
            .wrapping_add(1);
        search_index_path_rule_suggestion_request(
            &self.search_index.path_rule_input,
            self.search_index.path_rule_kind,
            &self.search_index_home_directory(),
            self.search_index.path_rule_suggestion_generation,
        )
    }

    fn search_index_path_rule_suggestion_request_matches(
        &self,
        request: &PathSuggestionRequest,
    ) -> bool {
        let Some(current) = search_index_path_rule_suggestion_request(
            &self.search_index.path_rule_input,
            self.search_index.path_rule_kind,
            &self.search_index_home_directory(),
            self.search_index.path_rule_suggestion_generation,
        ) else {
            return false;
        };

        request == &current
    }
}

fn search_index_path_rule_input_stabilization_command(
    request: PathSuggestionRequest,
) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(SEARCH_INDEX_PATH_RULE_SUGGESTION_DELAY).await;
            request
        },
        Message::SearchIndexPathRuleInputStabilized,
    )
}

fn search_index_path_rule_suggestion_request(
    input: &str,
    kind: SearchIndexPathRuleKind,
    home: &Path,
    generation: u64,
) -> Option<PathSuggestionRequest> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let suggestion_input = match kind {
        SearchIndexPathRuleKind::Indexed => expand_home_relative_input(trimmed, home),
        SearchIndexPathRuleKind::Excluded => {
            if !(trimmed.starts_with('~') || PathBuf::from(trimmed).is_absolute()) {
                return None;
            }
            expand_home_relative_input(trimmed, home)
        }
    };

    Some(PathSuggestionRequest {
        input: suggestion_input,
        current_dir: home.to_path_buf(),
        generation,
    })
}

fn expand_home_relative_input(input: &str, home: &Path) -> String {
    if input == "~" {
        return format!("{}/", home.to_string_lossy());
    }

    if let Some(rest) = input.strip_prefix("~/") {
        if rest.is_empty() {
            return format!("{}/", home.to_string_lossy());
        }
        return home.join(rest).to_string_lossy().into_owned();
    }

    input.to_owned()
}

#[cfg(test)]
mod tests {
    use super::search_index_path_rule_suggestion_request;
    use crate::model::SearchIndexPathRuleKind;
    use std::path::Path;

    #[test]
    fn indexed_path_suggestions_expand_home_relative_input() {
        let request = search_index_path_rule_suggestion_request(
            "~/Projects",
            SearchIndexPathRuleKind::Indexed,
            Path::new("/home/user"),
            7,
        )
        .expect("indexed request");

        assert_eq!(request.input, "/home/user/Projects");
        assert_eq!(request.current_dir, Path::new("/home/user"));
        assert_eq!(request.generation, 7);
    }

    #[test]
    fn global_exclude_patterns_do_not_request_path_suggestions() {
        let request = search_index_path_rule_suggestion_request(
            "node_modules/",
            SearchIndexPathRuleKind::Excluded,
            Path::new("/home/user"),
            3,
        );

        assert!(request.is_none());
    }

    #[test]
    fn absolute_exclude_paths_still_request_path_suggestions() {
        let request = search_index_path_rule_suggestion_request(
            "/var/cache",
            SearchIndexPathRuleKind::Excluded,
            Path::new("/home/user"),
            9,
        )
        .expect("absolute request");

        assert_eq!(request.input, "/var/cache");
        assert_eq!(request.generation, 9);
    }
}
