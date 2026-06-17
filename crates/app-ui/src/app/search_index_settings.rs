use std::collections::HashSet;
use std::path::{Path, PathBuf};

use file_core::{FileSearchIndexMode, FileSearchIndexStatus};
use iced::Task;

use super::FileBrowser;
use crate::commands::{
    clear_search_index_failures_command, remove_search_index_command, search_index_status_command,
};
use crate::config::{normalize_search_index_exclude_patterns, search_index_dir_for_root};
use crate::model::{Message, SettingsCategory, SidebarLocationKind};
use crate::operation_queue::QueuedFileOperation;

impl FileBrowser {
    pub(super) fn select_settings_category(&mut self, category: SettingsCategory) -> Task<Message> {
        self.selected_settings_category = category;
        if category == SettingsCategory::SearchIndex {
            self.prepare_search_index_settings()
        } else {
            Task::none()
        }
    }

    pub(super) fn prepare_search_index_settings_if_selected(&mut self) -> Task<Message> {
        self.sync_search_index_exclude_inputs_from_config_if_empty();
        if self.selected_settings_category == SettingsCategory::SearchIndex {
            self.refresh_search_index_statuses()
        } else {
            Task::none()
        }
    }

    pub(super) fn sync_search_index_exclude_inputs_from_config(&mut self) {
        self.search_index.exclude_pattern_inputs =
            self.user_config.search_index_exclude_patterns.clone();
    }

    pub(crate) fn search_index_setting_roots(&self) -> Vec<PathBuf> {
        let mut seen = HashSet::new();
        let mut roots = Vec::new();

        if !self.is_trash_view {
            push_unique_root(&mut roots, &mut seen, self.current_dir.clone());
        }
        if let Some(home) = self.home_search_index_root() {
            push_unique_root(&mut roots, &mut seen, home);
        }
        if let Some(search) = &self.search {
            push_unique_root(&mut roots, &mut seen, search.root.clone());
        }
        for root in self.search_index.statuses.keys() {
            push_unique_root(&mut roots, &mut seen, root.clone());
        }
        for root in &self.search_index.indexing_roots {
            push_unique_root(&mut roots, &mut seen, root.clone());
        }
        for root in self.search_index.errors.keys() {
            push_unique_root(&mut roots, &mut seen, root.clone());
        }

        roots
    }

    pub(crate) fn search_index_dir_for_settings_root(&self, root: &Path) -> PathBuf {
        search_index_dir_for_root(&self.search_index.base_dir, root)
    }

    pub(crate) fn search_index_exclude_patterns_have_changes(&self) -> bool {
        normalize_search_index_exclude_patterns(self.search_index.exclude_pattern_inputs.clone())
            != self.user_config.search_index_exclude_patterns
    }

    pub(crate) fn refresh_search_index_status_for_root(&mut self, root: PathBuf) -> Task<Message> {
        if self.search_index.status_loading_roots.contains(&root) {
            return Task::none();
        }

        let index_dir = self.search_index_dir_for_settings_root(&root);
        self.search_index.status_loading_roots.insert(root.clone());
        search_index_status_command(
            root,
            index_dir,
            self.options.clone(),
            self.user_config.search_index_exclude_patterns.clone(),
        )
    }

    pub(super) fn refresh_search_index_statuses(&mut self) -> Task<Message> {
        let roots = self.search_index_setting_roots();
        let tasks = roots
            .into_iter()
            .map(|root| self.refresh_search_index_status_for_root(root))
            .collect::<Vec<_>>();
        Task::batch(tasks)
    }

    pub(super) fn accept_search_index_status(
        &mut self,
        root: PathBuf,
        outcome: Result<FileSearchIndexStatus, String>,
    ) -> Task<Message> {
        self.search_index.status_loading_roots.remove(&root);
        match outcome {
            Ok(status) => {
                self.search_index.errors.remove(&root);
                self.search_index.statuses.insert(root.clone(), status);
            }
            Err(error) => {
                self.search_index.errors.insert(root.clone(), error);
            }
        }
        self.sync_search_index_status_for_active_search(&root);
        Task::none()
    }

    pub(super) fn request_search_index_manual_build(
        &mut self,
        root: PathBuf,
        mode: FileSearchIndexMode,
    ) -> Task<Message> {
        if self.search_index.indexing_roots.contains(&root) {
            return Task::none();
        }

        let index_dir = self.search_index_dir_for_settings_root(&root);
        self.search_index.indexing_roots.insert(root.clone());
        self.search_index.errors.remove(&root);
        self.sync_search_index_status_for_active_search(&root);

        self.enqueue_file_operation(QueuedFileOperation::BuildSearchIndex {
            root: root.clone(),
            index_dir,
            selected_paths: vec![root],
            include_hidden: self.options.include_hidden,
            exclude_patterns: self.user_config.search_index_exclude_patterns.clone(),
            mode,
        })
    }

    pub(super) fn request_search_index_removal(&mut self, root: PathBuf) -> Task<Message> {
        if self.search_index.indexing_roots.contains(&root)
            || self.search_index.status_loading_roots.contains(&root)
        {
            return Task::none();
        }

        let index_dir = self.search_index_dir_for_settings_root(&root);
        self.search_index.status_loading_roots.insert(root.clone());
        remove_search_index_command(
            root,
            index_dir,
            self.options.clone(),
            self.user_config.search_index_exclude_patterns.clone(),
        )
    }

    pub(super) fn request_search_index_failures_clear(&mut self, root: PathBuf) -> Task<Message> {
        if self.search_index.status_loading_roots.contains(&root) {
            return Task::none();
        }

        let index_dir = self.search_index_dir_for_settings_root(&root);
        self.search_index.status_loading_roots.insert(root.clone());
        clear_search_index_failures_command(
            root,
            index_dir,
            self.options.clone(),
            self.user_config.search_index_exclude_patterns.clone(),
        )
    }

    pub(super) fn update_search_index_exclude_pattern(
        &mut self,
        index: usize,
        pattern: String,
    ) -> Task<Message> {
        if let Some(input) = self.search_index.exclude_pattern_inputs.get_mut(index) {
            *input = pattern;
        }
        Task::none()
    }

    pub(super) fn add_search_index_exclude_pattern(&mut self) -> Task<Message> {
        self.search_index.exclude_pattern_inputs.push(String::new());
        Task::none()
    }

    pub(super) fn remove_search_index_exclude_pattern(&mut self, index: usize) -> Task<Message> {
        if index < self.search_index.exclude_pattern_inputs.len() {
            self.search_index.exclude_pattern_inputs.remove(index);
        }
        Task::none()
    }

    pub(super) fn save_search_index_exclude_patterns(&mut self) -> Task<Message> {
        let normalized = normalize_search_index_exclude_patterns(
            self.search_index.exclude_pattern_inputs.clone(),
        );
        self.search_index.exclude_pattern_inputs = normalized.clone();
        if normalized == self.user_config.search_index_exclude_patterns {
            return self.refresh_search_index_statuses();
        }

        self.user_config.search_index_exclude_patterns = normalized;
        Task::batch([
            self.persist_user_config_command(),
            self.refresh_search_index_statuses(),
        ])
    }

    fn prepare_search_index_settings(&mut self) -> Task<Message> {
        self.sync_search_index_exclude_inputs_from_config_if_empty();
        self.refresh_search_index_statuses()
    }

    fn sync_search_index_exclude_inputs_from_config_if_empty(&mut self) {
        if self.search_index.exclude_pattern_inputs.is_empty() {
            self.sync_search_index_exclude_inputs_from_config();
        }
    }

    fn home_search_index_root(&self) -> Option<PathBuf> {
        self.sidebar_locations
            .iter()
            .find(|location| location.kind == SidebarLocationKind::Home)
            .map(|location| location.path.clone())
    }

    fn sync_search_index_status_for_active_search(&mut self, root: &Path) {
        let is_indexing = self.search_index.indexing_roots.contains(root);
        let index_error = self.search_index.errors.get(root).cloned();
        if let Some(search) = self.active_search_mut_for_root(root) {
            search.is_indexing = is_indexing;
            search.index_error = index_error;
        }
    }
}

fn push_unique_root(roots: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, root: PathBuf) {
    if seen.insert(root.clone()) {
        roots.push(root);
    }
}
