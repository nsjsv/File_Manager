use iced::Task;

use crate::app::FileBrowser;
use crate::model::{
    Message, SearchIndexPathRuleKind, SearchIndexPathRuleSelection, SearchIndexSettingsSection,
};

impl FileBrowser {
    pub(crate) fn select_search_index_settings_section(
        &mut self,
        section: SearchIndexSettingsSection,
    ) -> Task<Message> {
        self.search_index.selected_settings_section = section;
        self.clear_search_index_path_rule_suggestions();
        self.search_index.path_rule_error = None;
        Task::none()
    }

    pub(crate) fn start_adding_search_index_indexed_path(&mut self) -> Task<Message> {
        self.start_adding_search_index_path_rule_with_kind(
            SearchIndexPathRuleKind::Indexed,
            "~/".to_owned(),
        );
        Task::none()
    }

    pub(crate) fn start_adding_search_index_exclude_rule(&mut self) -> Task<Message> {
        self.start_adding_search_index_path_rule_with_kind(
            SearchIndexPathRuleKind::Excluded,
            String::new(),
        );
        Task::none()
    }

    pub(crate) fn request_search_index_path_rule_edit(
        &mut self,
        selection: SearchIndexPathRuleSelection,
    ) -> Task<Message> {
        if !self.search_index_path_rule_selection_exists(&selection) {
            return Task::none();
        }

        self.start_modifying_search_index_path_rule(selection);
        self.clear_search_index_path_rule_suggestions();
        Task::none()
    }

    pub(crate) fn request_search_index_path_rule_removal(
        &mut self,
        selection: SearchIndexPathRuleSelection,
    ) -> Task<Message> {
        self.search_index.selected_path_rule = Some(selection);
        self.remove_selected_search_index_path_rule()
    }

    pub(crate) fn cancel_search_index_path_rule_edit(&mut self) -> Task<Message> {
        self.search_index.selected_path_rule = None;
        self.search_index.path_rule_editor = None;
        self.clear_search_index_path_rule_suggestions();
        self.search_index.path_rule_error = None;
        Task::none()
    }
}
