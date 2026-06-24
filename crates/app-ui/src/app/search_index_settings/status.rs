use std::path::PathBuf;

use file_index::FileSearchIndexStatus;
use iced::Task;

use crate::app::FileBrowser;
use crate::commands::search_index_status_command;
use crate::config::SearchBackendMode;
use crate::model::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchIndexStatusRefresh {
    SkipIfLoading,
    Force,
}

impl FileBrowser {
    pub(crate) fn refresh_search_index_status_for_root(&mut self, root: PathBuf) -> Task<Message> {
        self.refresh_search_index_status_for_root_with_mode(
            root,
            SearchIndexStatusRefresh::SkipIfLoading,
        )
    }

    pub(crate) fn force_refresh_search_index_status_for_root(
        &mut self,
        root: PathBuf,
    ) -> Task<Message> {
        self.search_index.status_generation = self.search_index.status_generation.wrapping_add(1);
        self.refresh_search_index_status_for_root_with_mode(root, SearchIndexStatusRefresh::Force)
    }

    fn refresh_search_index_status_for_root_with_mode(
        &mut self,
        root: PathBuf,
        refresh: SearchIndexStatusRefresh,
    ) -> Task<Message> {
        if self.user_config.search_mode != SearchBackendMode::Indexed {
            return Task::none();
        }
        if refresh == SearchIndexStatusRefresh::Force {
            self.search_index.status_loading_roots.remove(&root);
        } else if self.search_index.status_loading_roots.contains_key(&root) {
            return Task::none();
        }

        self.search_index
            .status_loading_roots
            .insert(root.clone(), self.search_index.status_generation);
        search_index_status_command(
            self.search_index.status_generation,
            root,
            self.user_config.clone(),
            self.search_index.profile_id.clone(),
        )
    }

    pub(crate) fn refresh_search_index_statuses(&mut self) -> Task<Message> {
        if self.user_config.search_mode != SearchBackendMode::Indexed {
            return Task::none();
        }
        let roots = self.search_index_setting_roots();
        let tasks = roots
            .into_iter()
            .map(|root| self.refresh_search_index_status_for_root(root))
            .collect::<Vec<_>>();
        Task::batch(tasks)
    }

    pub(crate) fn accept_search_index_status(
        &mut self,
        generation: u64,
        root: PathBuf,
        outcome: Result<FileSearchIndexStatus, String>,
    ) -> Task<Message> {
        if self.search_index.status_loading_roots.get(&root).copied() != Some(generation) {
            return Task::none();
        }
        self.search_index.status_loading_roots.remove(&root);
        match outcome {
            Ok(status) => {
                self.search_index.root_errors.remove(&root);
                self.search_index.statuses.insert(root.clone(), status);
            }
            Err(error) => {
                self.search_index.root_errors.insert(root.clone(), error);
            }
        }
        self.sync_search_index_status_for_active_search(&root);
        Task::none()
    }
}
