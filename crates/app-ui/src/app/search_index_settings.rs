use std::collections::HashSet;
use std::path::{Path, PathBuf};

use file_index::IndexProfile;
use file_index::{FileSearchIndexMode, MediaMetadataScope};
use iced::Task;

use super::FileBrowser;
use crate::commands::{
    clear_search_index_failures_command, default_search_index_profile, default_search_profile_id,
    remove_search_index_command, search_index_daemon_restart_command,
    search_index_daemon_status_command, search_index_profile_delete_command,
    search_index_profile_save_command,
};
use crate::config::normalize_search_index_exclude_patterns;
use crate::model::{
    Message, SearchIndexDaemonStatus, SearchIndexPathRuleEditMode, SearchIndexPathRuleKind,
    SearchIndexPathRuleSelection, SearchIndexProfileSaveReason, SettingsCategory,
};
use crate::operation_queue::QueuedFileOperation;

mod maintenance;
mod path_rules;
mod status;
#[cfg(test)]
mod tests;
use path_rules::{
    add_path_rule_to_search_index, remove_path_rule_from_search_index, root_is_inside_home,
    search_index_path_rule_input, search_index_path_rule_input_is_valid,
    update_path_rule_in_search_index, PathRuleChange,
};
pub(crate) use path_rules::{search_index_display_path, search_index_exclude_pattern_display_path};

impl FileBrowser {
    pub(super) fn select_settings_category(&mut self, category: SettingsCategory) -> Task<Message> {
        self.selected_settings_category = category;
        if category == SettingsCategory::SearchIndex {
            Task::batch([
                self.prepare_search_index_settings(),
                self.request_browser_session_save(),
            ])
        } else {
            self.request_browser_session_save()
        }
    }

    pub(super) fn prepare_search_index_settings_if_selected(&mut self) -> Task<Message> {
        self.sync_search_index_exclude_inputs_from_config_if_empty();
        if self.selected_settings_category == SettingsCategory::SearchIndex {
            self.prepare_search_index_settings()
        } else {
            Task::none()
        }
    }

    pub(super) fn sync_search_index_exclude_inputs_from_config(&mut self) {
        self.search_index.exclude_pattern_inputs =
            self.user_config.search_index_exclude_patterns.clone();
        self.search_index.sync_path_rule_order_with_current_rules();
    }

    pub(crate) fn search_index_setting_roots(&self) -> Vec<PathBuf> {
        let mut seen = HashSet::new();
        let mut roots = Vec::new();

        for root in &self.search_index.profile_roots {
            push_unique_root(&mut roots, &mut seen, root.clone());
        }
        for root in self.search_index.statuses.keys() {
            push_unique_root(&mut roots, &mut seen, root.clone());
        }
        for root in &self.search_index.indexing_roots {
            push_unique_root(&mut roots, &mut seen, root.clone());
        }
        for root in self.search_index.root_errors.keys() {
            push_unique_root(&mut roots, &mut seen, root.clone());
        }

        roots
    }

    pub(crate) fn search_index_home_directory(&self) -> PathBuf {
        if self.search_index.home_dir.as_os_str().is_empty() {
            self.current_dir.clone()
        } else {
            self.search_index.home_dir.clone()
        }
    }

    pub(crate) fn search_index_root_is_allowed(&self, root: &Path) -> bool {
        root_is_inside_home(root, &self.search_index_home_directory())
    }

    pub(crate) fn search_index_path_input_can_apply(&self) -> bool {
        search_index_path_rule_input_is_valid(
            &self.search_index.path_rule_input,
            self.search_index.path_rule_kind,
            &self.search_index.profile_roots,
            &self.search_index_home_directory(),
        )
    }

    pub(crate) fn selected_search_index_path_rule_exists(&self) -> bool {
        self.search_index
            .selected_path_rule
            .as_ref()
            .is_some_and(|selection| self.search_index_path_rule_selection_exists(selection))
    }

    fn search_index_path_rule_selection_exists(
        &self,
        selection: &SearchIndexPathRuleSelection,
    ) -> bool {
        match selection {
            SearchIndexPathRuleSelection::IndexedRoot(root) => {
                self.search_index.profile_roots.contains(root)
            }
            SearchIndexPathRuleSelection::ExcludePattern(index) => {
                *index < self.search_index.exclude_pattern_inputs.len()
            }
        }
    }

    pub(super) fn refresh_search_index_settings_statuses(&mut self) -> Task<Message> {
        Task::batch([
            self.refresh_search_index_daemon_status(),
            self.refresh_search_index_statuses(),
        ])
    }

    pub(super) fn refresh_search_index_daemon_status(&mut self) -> Task<Message> {
        if self.user_config.search_mode != crate::config::SearchBackendMode::Indexed {
            return Task::none();
        }
        if self.search_index.daemon_status_loading {
            return Task::none();
        }
        self.search_index.daemon_status_loading = true;
        search_index_daemon_status_command(self.user_config.clone())
    }

    pub(super) fn request_search_index_daemon_restart(&mut self) -> Task<Message> {
        if self.user_config.search_mode != crate::config::SearchBackendMode::Indexed {
            return Task::none();
        }
        if self.search_index.daemon_status_loading {
            return Task::none();
        }
        self.search_index.daemon_status_loading = true;
        self.search_index.service_generation = self.search_index.service_generation.wrapping_add(1);
        search_index_daemon_restart_command(self.user_config.clone())
    }

    pub(super) fn accept_search_index_daemon_status(
        &mut self,
        outcome: Result<SearchIndexDaemonStatus, String>,
    ) -> Task<Message> {
        self.search_index.daemon_status_loading = false;
        self.search_index.daemon_status = Some(match outcome {
            Ok(status) => status,
            Err(error) => SearchIndexDaemonStatus::Unreachable(error),
        });
        Task::none()
    }

    pub(super) fn request_search_index_manual_build(
        &mut self,
        root: PathBuf,
        mode: FileSearchIndexMode,
    ) -> Task<Message> {
        if self.user_config.search_mode != crate::config::SearchBackendMode::Indexed {
            return Task::none();
        }
        if !self.search_index_root_is_allowed(&root) {
            self.search_index.profile_error =
                Some("Only paths under your home directory can be indexed.".to_owned());
            return Task::none();
        }
        if self.search_index.indexing_roots.contains(&root) {
            return Task::none();
        }

        self.search_index.indexing_roots.insert(root.clone());
        self.search_index.root_errors.remove(&root);
        self.sync_search_index_status_for_active_search(&root);
        if !self.search_index.profile_roots.contains(&root) {
            self.search_index.sync_path_rule_order_with_current_rules();
            self.search_index.profile_roots.push(root.clone());
            self.search_index.append_path_rule_to_order(
                &SearchIndexPathRuleSelection::IndexedRoot(root.clone()),
            );
        }

        Task::batch([
            self.save_current_search_index_profile(),
            self.enqueue_file_operation(QueuedFileOperation::BuildSearchIndex {
                profile_id: self.search_index.profile_id.clone(),
                root: root.clone(),
                index_base_dir: self.search_index.base_dir.clone(),
                selected_paths: vec![root],
                mode,
            }),
        ])
    }

    pub(super) fn request_search_index_removal(&mut self, root: PathBuf) -> Task<Message> {
        if self.user_config.search_mode != crate::config::SearchBackendMode::Indexed {
            return Task::none();
        }
        if self.search_index.indexing_roots.contains(&root)
            || self.search_index.status_loading_roots.contains_key(&root)
        {
            return Task::none();
        }

        self.search_index.status_generation = self.search_index.status_generation.wrapping_add(1);
        self.search_index
            .status_loading_roots
            .insert(root.clone(), self.search_index.status_generation);
        remove_search_index_command(
            self.search_index.status_generation,
            root,
            self.user_config.clone(),
            self.search_index.profile_id.clone(),
        )
    }

    pub(super) fn request_search_index_failures_clear(&mut self, root: PathBuf) -> Task<Message> {
        if self.user_config.search_mode != crate::config::SearchBackendMode::Indexed {
            return Task::none();
        }
        if self.search_index.status_loading_roots.contains_key(&root) {
            return Task::none();
        }

        self.search_index.status_generation = self.search_index.status_generation.wrapping_add(1);
        self.search_index
            .status_loading_roots
            .insert(root.clone(), self.search_index.status_generation);
        clear_search_index_failures_command(
            self.search_index.status_generation,
            root,
            self.user_config.clone(),
            self.search_index.profile_id.clone(),
        )
    }

    pub(super) fn save_search_index_exclude_patterns(&mut self) -> Task<Message> {
        let normalized = normalize_search_index_exclude_patterns(
            self.search_index.exclude_pattern_inputs.clone(),
        );
        self.search_index.exclude_pattern_inputs = normalized.clone();
        self.search_index.sync_path_rule_order_with_current_rules();
        if normalized == self.user_config.search_index_exclude_patterns {
            return self.refresh_search_index_statuses();
        }

        self.user_config.search_index_exclude_patterns = normalized;
        Task::batch([
            self.persist_user_config_command(),
            self.save_current_search_index_profile(),
            self.refresh_search_index_statuses(),
        ])
    }

    pub(super) fn select_search_index_directory_error_policy(
        &mut self,
        policy: file_index::DirectoryErrorPolicy,
    ) -> Task<Message> {
        if self.search_index.directory_error_policy == policy {
            return Task::none();
        }
        self.search_index.directory_error_policy = policy;
        self.user_config.search_index_directory_error_policy = policy;
        Task::batch([
            self.persist_user_config_command(),
            self.save_current_search_index_profile(),
            self.refresh_search_index_statuses(),
        ])
    }

    pub(super) fn toggle_search_index_content(&mut self, enabled: bool) -> Task<Message> {
        if self.search_index.content_index_enabled == enabled {
            return Task::none();
        }
        self.search_index.content_index_enabled = enabled;
        self.user_config.search_index_content_enabled = enabled;
        Task::batch([
            self.persist_user_config_command(),
            self.save_current_search_index_profile(),
        ])
    }

    pub(super) fn select_search_index_media_scope(
        &mut self,
        scope: MediaMetadataScope,
    ) -> Task<Message> {
        if self.search_index.media_metadata_scope == scope {
            return Task::none();
        }
        self.search_index.media_metadata_scope = scope;
        self.user_config.search_index_media_scope = scope;
        Task::batch([
            self.persist_user_config_command(),
            self.save_current_search_index_profile(),
        ])
    }

    pub(super) fn accept_search_index_profile(
        &mut self,
        outcome: Result<Option<IndexProfile>, String>,
    ) -> Task<Message> {
        self.search_index.profile_loading = false;
        match outcome {
            Ok(Some(mut profile)) => {
                let home = self.search_index_home_directory();
                let loaded_roots = profile.roots.clone();
                let profile_missing_configured_excludes = profile.exclude_patterns.is_empty()
                    && !self.user_config.search_index_exclude_patterns.is_empty();
                if profile_missing_configured_excludes {
                    profile.exclude_patterns =
                        self.user_config.search_index_exclude_patterns.clone();
                }
                profile
                    .roots
                    .retain(|root| root_is_inside_home(root, &home));
                self.user_config.search_index_exclude_patterns = profile.exclude_patterns.clone();
                self.search_index.exclude_pattern_inputs = profile.exclude_patterns.clone();
                self.user_config.search_index_directory_error_policy =
                    profile.directory_error_policy;
                self.user_config.search_index_content_enabled = profile.content.enabled;
                self.user_config.search_index_media_scope = profile.media.scope;
                self.search_index.apply_profile(&profile);
                let save_profile_task =
                    if profile.roots != loaded_roots || profile_missing_configured_excludes {
                        self.save_current_search_index_profile()
                    } else {
                        Task::none()
                    };
                self.persist_user_config_command()
                    .chain(save_profile_task)
                    .chain(self.refresh_search_index_statuses())
            }
            Ok(None) => {
                self.search_index.profile_error = None;
                Task::none()
            }
            Err(error) => {
                self.search_index.profile_error = Some(error);
                Task::none()
            }
        }
    }

    pub(super) fn accept_search_index_profile_save(
        &mut self,
        reason: SearchIndexProfileSaveReason,
        outcome: Result<IndexProfile, String>,
    ) -> Task<Message> {
        match outcome {
            Ok(profile) => {
                self.search_index.apply_profile(&profile);
                let startup_index_task =
                    if reason == SearchIndexProfileSaveReason::StartupIndexSetup {
                        self.enqueue_pending_startup_index_builds()
                    } else {
                        Task::none()
                    };
                Task::batch([startup_index_task, self.refresh_search_index_statuses()])
            }
            Err(error) => {
                if reason == SearchIndexProfileSaveReason::StartupIndexSetup {
                    self.search_index.pending_startup_index_builds.clear();
                }
                self.search_index.profile_error = Some(error);
                Task::none()
            }
        }
    }

    pub(super) fn accept_search_index_profile_delete(
        &mut self,
        outcome: Result<String, String>,
    ) -> Task<Message> {
        match outcome {
            Ok(_) => {
                self.search_index.profile_roots.clear();
                self.search_index.statuses.clear();
                self.search_index.root_errors.clear();
                self.search_index.sync_path_rule_order_with_current_rules();
                self.search_index.profile_error = None;
                self.search_index.service_generation =
                    self.search_index.service_generation.wrapping_add(1);
            }
            Err(error) => {
                self.search_index.profile_error = Some(error);
            }
        }
        Task::none()
    }

    pub(super) fn select_search_index_path_rule(
        &mut self,
        selection: SearchIndexPathRuleSelection,
    ) -> Task<Message> {
        let task = self.commit_search_index_path_rule_editor();
        if self.search_index.path_rule_editor.is_some() {
            return task;
        }
        self.search_index.selected_path_rule = Some(selection);
        task
    }

    pub(super) fn select_search_index_path_rule_kind(
        &mut self,
        kind: SearchIndexPathRuleKind,
    ) -> Task<Message> {
        self.search_index.path_rule_kind = kind;
        Task::none()
    }

    pub(super) fn change_search_index_path_rule_kind(
        &mut self,
        selection: SearchIndexPathRuleSelection,
        kind: SearchIndexPathRuleKind,
    ) -> Task<Message> {
        let commit_task = self.commit_search_index_path_rule_editor();
        if self.search_index.path_rule_editor.is_some()
            || !self.search_index_path_rule_selection_exists(&selection)
        {
            return commit_task;
        }
        if search_index_path_rule_selection_kind(&selection) == kind {
            return commit_task;
        }

        self.search_index.sync_path_rule_order_with_current_rules();
        let old_order_entry = self.search_index.path_rule_order_entry(&selection);
        let home = self.search_index_home_directory();
        let Some(input) = search_index_path_rule_input(&self.search_index, &selection, &home)
        else {
            return commit_task;
        };
        let was_selected = self.search_index.selected_path_rule.as_ref() == Some(&selection);
        let change = match update_path_rule_in_search_index(
            &mut self.search_index.profile_roots,
            &mut self.search_index.exclude_pattern_inputs,
            &mut self.search_index.statuses,
            &mut self.search_index.root_errors,
            &selection,
            &input,
            kind,
            &home,
        ) {
            Ok(change) => change,
            Err(error) => {
                self.search_index.profile_error = Some(error);
                return commit_task;
            }
        };
        self.search_index
            .replace_path_rule_order_entry(old_order_entry, &change.selection);
        self.update_selection_after_path_rule_kind_change(&selection, &change, was_selected);
        self.search_index.profile_error = None;
        commit_task.chain(self.save_search_index_path_rule_change(&change))
    }

    pub(super) fn update_search_index_path_rule_input(&mut self, input: String) -> Task<Message> {
        self.search_index.path_rule_input = input;
        self.search_index.profile_error = None;
        Task::none()
    }

    pub(super) fn add_search_index_path_rule(&mut self) -> Task<Message> {
        match self.search_index.path_rule_editor.clone() {
            Some(SearchIndexPathRuleEditMode::Adding) => {
                return self.commit_search_index_path_rule_editor();
            }
            Some(SearchIndexPathRuleEditMode::Modifying(_)) => {
                let task = self.commit_search_index_path_rule_editor();
                if self.search_index.path_rule_editor.is_some() {
                    return task;
                }
                self.start_adding_search_index_path_rule();
                return task;
            }
            None => {}
        }

        self.start_adding_search_index_path_rule();
        Task::none()
    }

    pub(super) fn commit_search_index_path_rule_editor(&mut self) -> Task<Message> {
        match self.search_index.path_rule_editor.clone() {
            Some(SearchIndexPathRuleEditMode::Adding) => self.commit_new_search_index_path_rule(),
            Some(SearchIndexPathRuleEditMode::Modifying(selection)) => self
                .apply_search_index_path_rule_change(
                    selection,
                    self.search_index.path_rule_input.clone(),
                    self.search_index.path_rule_kind,
                ),
            None => Task::none(),
        }
    }

    fn commit_new_search_index_path_rule(&mut self) -> Task<Message> {
        self.search_index.sync_path_rule_order_with_current_rules();
        let home = self.search_index_home_directory();
        let change = match add_path_rule_to_search_index(
            &mut self.search_index.profile_roots,
            &mut self.search_index.exclude_pattern_inputs,
            &self.search_index.path_rule_input,
            self.search_index.path_rule_kind,
            &home,
        ) {
            Ok(change) => change,
            Err(error) => {
                self.search_index.profile_error = Some(error);
                return Task::none();
            }
        };
        self.search_index
            .append_path_rule_to_order(&change.selection);
        let task = self.save_search_index_path_rule_change(&change);
        self.sync_path_rule_editor_to_selection(change.selection);
        task
    }

    pub(super) fn remove_selected_search_index_path_rule(&mut self) -> Task<Message> {
        let commit_task = self.commit_search_index_path_rule_editor();
        if self.search_index.path_rule_editor.is_some() {
            return commit_task;
        }
        let Some(selection) = self.search_index.selected_path_rule.take() else {
            return commit_task;
        };
        self.search_index.sync_path_rule_order_with_current_rules();
        let removed_order_entry = self.search_index.path_rule_order_entry(&selection);
        self.search_index.path_rule_editor = None;
        let change = remove_path_rule_from_search_index(
            &mut self.search_index.profile_roots,
            &mut self.search_index.exclude_pattern_inputs,
            &mut self.search_index.statuses,
            &mut self.search_index.root_errors,
            &selection,
        );
        self.search_index
            .remove_path_rule_order_entry(removed_order_entry);
        let remove_task = match (change.roots_changed, change.excludes_changed) {
            (_, true) => self.save_search_index_exclude_patterns(),
            (true, false) => self.save_current_search_index_profile(),
            (false, false) => Task::none(),
        };
        commit_task.chain(remove_task)
    }

    pub(super) fn update_selected_search_index_path_rule(&mut self) -> Task<Message> {
        if matches!(
            self.search_index.path_rule_editor,
            Some(SearchIndexPathRuleEditMode::Adding)
        ) {
            return self.commit_search_index_path_rule_editor();
        }

        if !self.selected_search_index_path_rule_exists() {
            return self.add_search_index_path_rule();
        }

        let Some(selection) = self.search_index.selected_path_rule.clone() else {
            return Task::none();
        };
        if !matches!(
            &self.search_index.path_rule_editor,
            Some(SearchIndexPathRuleEditMode::Modifying(editing)) if editing == &selection
        ) {
            self.start_modifying_search_index_path_rule(selection);
            return Task::none();
        }

        self.apply_search_index_path_rule_change(
            selection,
            self.search_index.path_rule_input.clone(),
            self.search_index.path_rule_kind,
        )
    }

    fn apply_search_index_path_rule_change(
        &mut self,
        selection: SearchIndexPathRuleSelection,
        input: String,
        kind: SearchIndexPathRuleKind,
    ) -> Task<Message> {
        self.search_index.sync_path_rule_order_with_current_rules();
        let old_order_entry = self.search_index.path_rule_order_entry(&selection);
        let home = self.search_index_home_directory();
        let change = match update_path_rule_in_search_index(
            &mut self.search_index.profile_roots,
            &mut self.search_index.exclude_pattern_inputs,
            &mut self.search_index.statuses,
            &mut self.search_index.root_errors,
            &selection,
            &input,
            kind,
            &home,
        ) {
            Ok(change) => change,
            Err(error) => {
                self.search_index.profile_error = Some(error);
                return Task::none();
            }
        };
        self.search_index
            .replace_path_rule_order_entry(old_order_entry, &change.selection);
        let task = self.save_search_index_path_rule_change(&change);
        self.sync_path_rule_editor_to_selection(change.selection);
        task
    }

    fn save_search_index_path_rule_change(&mut self, change: &PathRuleChange) -> Task<Message> {
        match (change.roots_changed, change.excludes_changed) {
            (_, true) => self.save_search_index_exclude_patterns(),
            (true, false) => self.save_current_search_index_profile(),
            (false, false) => Task::none(),
        }
    }

    fn update_selection_after_path_rule_kind_change(
        &mut self,
        selection: &SearchIndexPathRuleSelection,
        change: &PathRuleChange,
        was_selected: bool,
    ) {
        if was_selected {
            self.sync_path_rule_editor_to_selection(change.selection.clone());
        } else if change.excludes_changed
            && matches!(
                self.search_index.selected_path_rule.as_ref(),
                Some(SearchIndexPathRuleSelection::ExcludePattern(_))
            )
        {
            self.search_index.selected_path_rule = None;
        }
        if self.search_index.selected_path_rule.as_ref() == Some(selection)
            && !self.selected_search_index_path_rule_exists()
        {
            self.search_index.selected_path_rule = None;
        }
    }

    fn sync_path_rule_editor_to_selection(&mut self, selection: SearchIndexPathRuleSelection) {
        let home = self.search_index_home_directory();
        let input = search_index_path_rule_input(&self.search_index, &selection, &home)
            .unwrap_or_else(|| "~".to_owned());
        self.search_index.path_rule_kind = match selection {
            SearchIndexPathRuleSelection::IndexedRoot(_) => SearchIndexPathRuleKind::Indexed,
            SearchIndexPathRuleSelection::ExcludePattern(_) => SearchIndexPathRuleKind::Excluded,
        };
        self.search_index.path_rule_input = input;
        self.search_index.selected_path_rule = Some(selection);
        self.search_index.path_rule_editor = None;
        self.search_index.profile_error = None;
    }

    fn start_adding_search_index_path_rule(&mut self) {
        self.search_index.selected_path_rule = None;
        self.search_index.path_rule_editor = Some(SearchIndexPathRuleEditMode::Adding);
        self.search_index.path_rule_kind = SearchIndexPathRuleKind::Indexed;
        self.search_index.path_rule_input = "~".to_owned();
        self.search_index.profile_error = None;
    }

    fn start_modifying_search_index_path_rule(&mut self, selection: SearchIndexPathRuleSelection) {
        let home = self.search_index_home_directory();
        let input = search_index_path_rule_input(&self.search_index, &selection, &home)
            .unwrap_or_else(|| "~".to_owned());
        self.search_index.path_rule_kind = match selection {
            SearchIndexPathRuleSelection::IndexedRoot(_) => SearchIndexPathRuleKind::Indexed,
            SearchIndexPathRuleSelection::ExcludePattern(_) => SearchIndexPathRuleKind::Excluded,
        };
        self.search_index.path_rule_input = input;
        self.search_index.selected_path_rule = Some(selection.clone());
        self.search_index.path_rule_editor =
            Some(SearchIndexPathRuleEditMode::Modifying(selection));
        self.search_index.profile_error = None;
    }

    pub(super) fn request_search_index_profile_delete(&mut self) -> Task<Message> {
        if self.user_config.search_mode != crate::config::SearchBackendMode::Indexed {
            return Task::none();
        }
        self.search_index.profile_loading = true;
        search_index_profile_delete_command(
            self.search_index.profile_id.clone(),
            self.user_config.clone(),
        )
    }

    fn prepare_search_index_settings(&mut self) -> Task<Message> {
        self.sync_search_index_exclude_inputs_from_config_if_empty();
        if self.user_config.search_mode != crate::config::SearchBackendMode::Indexed {
            return Task::none();
        }
        Task::batch([
            self.refresh_search_index_daemon_status(),
            self.load_search_index_profile_command(),
            self.refresh_search_index_statuses(),
        ])
    }

    fn sync_search_index_exclude_inputs_from_config_if_empty(&mut self) {
        if self.search_index.exclude_pattern_inputs.is_empty() {
            self.sync_search_index_exclude_inputs_from_config();
        }
    }

    pub(super) fn sync_search_index_status_for_active_search(&mut self, root: &Path) {
        let is_indexing = self.search_index.indexing_roots.contains(root);
        let index_error = self.search_index.root_errors.get(root).cloned();
        if let Some(search) = self.active_search_mut_for_root(root) {
            search.is_indexing = is_indexing;
            search.index_error = index_error;
        }
    }

    pub(super) fn load_search_index_profile_command(&mut self) -> Task<Message> {
        if self.user_config.search_mode != crate::config::SearchBackendMode::Indexed {
            return Task::none();
        }
        if self.search_index.profile_loading {
            return Task::none();
        }
        self.search_index.profile_loading = true;
        crate::commands::search_index_profile_load_command(self.user_config.clone())
    }

    fn save_current_search_index_profile(&mut self) -> Task<Message> {
        let roots = self.search_index.profile_roots.clone();
        let mut profile = default_search_index_profile(&self.user_config, roots);
        profile.include_hidden = self.options.include_hidden;
        profile.id = self.search_index.profile_id.clone();
        if profile.id.is_empty() {
            profile.id = default_search_profile_id().to_owned();
        }
        search_index_profile_save_command(
            profile,
            self.user_config.clone(),
            SearchIndexProfileSaveReason::General,
        )
    }
}

fn push_unique_root(roots: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, root: PathBuf) {
    if seen.insert(root.clone()) {
        roots.push(root);
    }
}

fn search_index_path_rule_selection_kind(
    selection: &SearchIndexPathRuleSelection,
) -> SearchIndexPathRuleKind {
    match selection {
        SearchIndexPathRuleSelection::IndexedRoot(_) => SearchIndexPathRuleKind::Indexed,
        SearchIndexPathRuleSelection::ExcludePattern(_) => SearchIndexPathRuleKind::Excluded,
    }
}
