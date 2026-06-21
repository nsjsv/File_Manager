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
            Message::SearchIndexStatusLoaded(root, outcome) => {
                self.accept_search_index_status(root, outcome)
            }
            Message::SearchIndexProfileLoaded(outcome) => self.accept_search_index_profile(outcome),
            Message::SearchIndexProfileSaved(outcome) => {
                self.accept_search_index_profile_save(outcome)
            }
            Message::SearchIndexProfileDeleted(outcome) => {
                self.accept_search_index_profile_delete(outcome)
            }
            Message::SearchIndexDaemonStatusLoaded(outcome)
            | Message::SearchIndexDaemonRestarted(outcome) => {
                self.accept_search_index_daemon_status(outcome)
            }
            Message::SearchIndexDaemonRestartRequested => {
                self.request_search_index_daemon_restart()
            }
            Message::SearchIndexMaintenanceEvent(generation, event) => {
                self.accept_search_index_maintenance_event(generation, event)
            }
            Message::SearchIndexMaintenanceUpdated(generation, outcome) => {
                self.accept_search_index_maintenance_update(generation, outcome)
            }
            Message::SearchIndexStatusRefreshRequested => {
                self.refresh_search_index_settings_statuses()
            }
            Message::SearchIndexManualBuildRequested(root, mode) => {
                self.request_search_index_manual_build(root, mode)
            }
            Message::SearchIndexRemoveRequested(root) => self.request_search_index_removal(root),
            Message::SearchIndexProfileDeleteRequested => {
                self.request_search_index_profile_delete()
            }
            Message::SearchIndexMaintenancePauseToggled => {
                self.toggle_search_index_maintenance_pause()
            }
            Message::SearchIndexFailuresClearRequested(root) => {
                self.request_search_index_failures_clear(root)
            }
            Message::SearchIndexPathRuleSelected(selection) => {
                self.select_search_index_path_rule(selection)
            }
            Message::SearchIndexPathRuleKindChanged(selection, kind) => {
                self.change_search_index_path_rule_kind(selection, kind)
            }
            Message::SearchIndexPathRuleKindSelected(kind) => {
                self.select_search_index_path_rule_kind(kind)
            }
            Message::SearchIndexPathRuleInputChanged(input) => {
                self.update_search_index_path_rule_input(input)
            }
            Message::SearchIndexPathRuleEditorCommitted => {
                self.commit_search_index_path_rule_editor()
            }
            Message::SearchIndexPathRuleAdded => self.add_search_index_path_rule(),
            Message::SearchIndexPathRuleRemoved => self.remove_selected_search_index_path_rule(),
            Message::SearchIndexPathRuleUpdated => self.update_selected_search_index_path_rule(),
            Message::SearchIndexDirectoryErrorPolicySelected(policy) => {
                self.select_search_index_directory_error_policy(policy)
            }
            Message::SearchIndexContentEnabledToggled(enabled) => {
                self.toggle_search_index_content(enabled)
            }
            Message::SearchIndexMediaEnabledToggled(enabled) => {
                self.toggle_search_index_media(enabled)
            }
            Message::SearchBackendModeSelected(mode) => self.select_search_backend_mode(mode),
            Message::SearchModePromptSimpleSelected => self.select_simple_search_from_prompt(),
            Message::SearchModePromptIndexedSelected => self.select_indexed_search_from_prompt(),
            Message::SearchMatchSelected(path) => self.activate_search_match(path),
            Message::SearchActivated => self.activate_selected_search_match(),
            _ => Task::none(),
        }
    }
}

fn search_index_message_commits_path_rule_editor(message: &Message) -> bool {
    matches!(
        message,
        Message::SearchIndexStatusRefreshRequested
            | Message::SearchIndexManualBuildRequested(_, _)
            | Message::SearchIndexRemoveRequested(_)
            | Message::SearchIndexProfileDeleteRequested
            | Message::SearchIndexDaemonRestartRequested
            | Message::SearchIndexMaintenancePauseToggled
            | Message::SearchIndexFailuresClearRequested(_)
            | Message::SearchIndexDirectoryErrorPolicySelected(_)
            | Message::SearchIndexContentEnabledToggled(_)
            | Message::SearchIndexMediaEnabledToggled(_)
            | Message::SearchBackendModeSelected(_)
    )
}
