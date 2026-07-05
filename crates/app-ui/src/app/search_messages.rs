use iced::Task;

use super::FileBrowser;
use crate::model::Message;

impl FileBrowser {
    pub(super) fn handle_search_message(&mut self, message: Message) -> Task<Message> {
        if search_index_message_commits_path_rule_editor(&message) {
            let task = self.commit_search_index_path_rule_editor();
            if self.search_index.path_rule_editor.is_some() {
                return task;
            }
            return task.chain(self.handle_search_message_without_path_rule_commit(message));
        }
        self.handle_search_message_without_path_rule_commit(message)
    }

    fn handle_search_message_without_path_rule_commit(
        &mut self,
        message: Message,
    ) -> Task<Message> {
        match message {
            Message::SearchInputChanged(query) => self.update_search_query(query),
            Message::SearchModeSelected(mode) => self.select_search_mode(mode),
            Message::SearchInputStabilized(request) => self.load_stable_search_matches(request),
            Message::SearchFocusRequested => {
                if self.search.is_some() {
                    super::search::focus_search_input_command()
                } else {
                    Task::none()
                }
            }
            Message::SearchMatchesLoaded(request, search) => {
                self.accept_search_matches(request, search)
            }
            Message::SearchIndexBuilt(root, outcome) => self.accept_search_index(root, outcome),
            Message::SearchIndexStatusLoaded(generation, root, outcome) => {
                self.accept_search_index_status(generation, root, outcome)
            }
            Message::SearchIndexProfileLoaded(outcome) => self.accept_search_index_profile(outcome),
            Message::SearchIndexProfileSaved(reason, outcome) => {
                self.accept_search_index_profile_save(reason, outcome)
            }
            Message::SearchIndexProfileDeleted(outcome) => {
                self.accept_search_index_profile_delete(outcome)
            }
            Message::SearchIndexDaemonStatusLoaded(outcome)
            | Message::SearchIndexDaemonRestarted(outcome) => {
                self.accept_search_index_daemon_status(outcome)
            }
            Message::SearchIndexSettingsSectionSelected(section) => {
                self.select_search_index_settings_section(section)
            }
            Message::SearchIndexErrorCopyRequested(target) => self.copy_search_index_error(target),
            Message::SearchIndexDaemonRestartRequested => {
                self.request_search_index_daemon_restart()
            }
            Message::SearchIndexStatusRefreshRequested => {
                self.refresh_search_index_settings_statuses()
            }
            Message::SearchIndexManualBuildRequested(root) => {
                self.request_search_index_manual_build(root)
            }
            Message::SearchIndexRemoveRequested(root) => self.request_search_index_removal(root),
            Message::SearchIndexProfileDeleteRequested => {
                self.request_search_index_profile_delete()
            }
            Message::SearchIndexFailuresClearRequested(root) => {
                self.request_search_index_failures_clear(root)
            }
            Message::SearchIndexPathRuleSelected(selection) => {
                self.select_search_index_path_rule(selection)
            }
            Message::SearchIndexIndexedPathAddRequested => {
                self.start_adding_search_index_indexed_path()
            }
            Message::SearchIndexExcludeRuleAddRequested => {
                self.start_adding_search_index_exclude_rule()
            }
            Message::SearchIndexPathRuleEditRequested(selection) => {
                self.request_search_index_path_rule_edit(selection)
            }
            Message::SearchIndexPathRuleRemoveRequested(selection) => {
                self.request_search_index_path_rule_removal(selection)
            }
            Message::SearchIndexPathRuleInputChanged(input) => {
                self.update_search_index_path_rule_input(input)
            }
            Message::SearchIndexPathRuleInputStabilized(request) => {
                self.load_stable_search_index_path_rule_suggestions(request)
            }
            Message::SearchIndexPathRuleSuggestionsLoaded(request, suggestions) => {
                self.accept_search_index_path_rule_suggestions(request, suggestions)
            }
            Message::SearchIndexPathRuleSuggestionSelected(path) => {
                self.select_search_index_path_rule_suggestion(path)
            }
            Message::SearchIndexPathRuleEditorCommitted => {
                self.commit_search_index_path_rule_editor()
            }
            Message::SearchIndexPathRuleEditCanceled => self.cancel_search_index_path_rule_edit(),
            Message::SearchIndexPathRuleAdded => self.add_search_index_path_rule(),
            Message::SearchIndexPathRuleUpdated => self.update_selected_search_index_path_rule(),
            Message::SearchIndexDirectoryErrorPolicySelected(policy) => {
                self.select_search_index_directory_error_policy(policy)
            }
            Message::SearchIndexMediaScopeSelected(scope) => {
                self.select_search_index_media_scope(scope)
            }
            Message::SearchBackendModeSelected(mode) => self.select_search_backend_mode(mode),
            Message::SearchModePromptModeSelected(mode) => {
                self.select_search_mode_prompt_mode(mode)
            }
            Message::SearchModePromptNextPressed => self.accept_search_mode_prompt(),
            Message::SearchMatchSelected(path) => self.activate_search_match(path),
            Message::SearchActivated => self.activate_selected_search_match(),
            _ => Task::none(),
        }
    }
}

fn search_index_message_commits_path_rule_editor(message: &Message) -> bool {
    matches!(
        message,
        Message::SearchIndexSettingsSectionSelected(_)
            | Message::SearchIndexIndexedPathAddRequested
            | Message::SearchIndexExcludeRuleAddRequested
            | Message::SearchIndexPathRuleEditRequested(_)
            | Message::SearchIndexPathRuleRemoveRequested(_)
            | Message::SearchIndexStatusRefreshRequested
            | Message::SearchIndexManualBuildRequested(_)
            | Message::SearchIndexRemoveRequested(_)
            | Message::SearchIndexProfileDeleteRequested
            | Message::SearchIndexDaemonRestartRequested
            | Message::SearchIndexFailuresClearRequested(_)
            | Message::SearchIndexDirectoryErrorPolicySelected(_)
            | Message::SearchIndexMediaScopeSelected(_)
            | Message::SearchBackendModeSelected(_)
    )
}
