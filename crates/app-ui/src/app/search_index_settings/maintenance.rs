use file_index::IndexServiceEvent;
use iced::Task;

use crate::app::FileBrowser;
use crate::config::SearchBackendMode;
use crate::model::Message;

impl FileBrowser {
    pub(in crate::app) fn toggle_search_index_maintenance_pause(&mut self) -> Task<Message> {
        if self.user_config.search_mode != SearchBackendMode::Indexed {
            return Task::none();
        }
        self.search_index.maintenance_paused = !self.search_index.maintenance_paused;
        self.search_index.service_generation = self.search_index.service_generation.wrapping_add(1);
        crate::commands::search_index_maintenance_pause_command(
            self.search_index.profile_id.clone(),
            self.user_config.clone(),
            self.search_index.maintenance_paused,
            self.search_index.service_generation,
        )
    }

    pub(in crate::app) fn accept_search_index_maintenance_event(
        &mut self,
        generation: u64,
        event: IndexServiceEvent,
    ) -> Task<Message> {
        if self.user_config.search_mode != SearchBackendMode::Indexed {
            return Task::none();
        }
        if generation != self.search_index.service_generation {
            return Task::none();
        }
        match event {
            IndexServiceEvent::WatchStarted { root, .. } => {
                self.search_index.errors.remove(&root);
                self.refresh_search_index_status_for_root(root)
            }
            IndexServiceEvent::WatchFailed { root, message, .. } => {
                self.search_index.errors.insert(root, message);
                Task::none()
            }
            IndexServiceEvent::IncrementalUpdateStarted { root, .. } => {
                self.search_index.indexing_roots.insert(root.clone());
                self.sync_search_index_status_for_active_search(&root);
                Task::none()
            }
            IndexServiceEvent::IncrementalUpdateFinished { outcome, .. } => {
                let root = outcome.root.clone();
                self.search_index.indexing_roots.remove(&root);
                self.search_index.errors.remove(&root);
                self.sync_search_index_status_for_active_search(&root);
                Task::batch([
                    self.refresh_search_index_status_for_root(root),
                    self.load_search_matches(),
                ])
            }
            IndexServiceEvent::IncrementalUpdateFailed { root, message, .. } => {
                self.search_index.indexing_roots.remove(&root);
                self.search_index.errors.insert(root.clone(), message);
                self.sync_search_index_status_for_active_search(&root);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    pub(in crate::app) fn accept_search_index_maintenance_update(
        &mut self,
        generation: u64,
        outcome: Result<bool, String>,
    ) -> Task<Message> {
        if self.user_config.search_mode != SearchBackendMode::Indexed {
            return Task::none();
        }
        if generation != self.search_index.service_generation {
            return Task::none();
        }
        match outcome {
            Ok(paused) => {
                self.search_index.maintenance_paused = paused;
                self.search_index.profile_error = None;
            }
            Err(error) => {
                self.search_index.profile_error = Some(error);
            }
        }
        Task::none()
    }
}
