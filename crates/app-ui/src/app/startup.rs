use std::path::PathBuf;

use iced::Task;

use super::paths::path_text;
use super::FileBrowser;
use crate::commands::{load_directory_command, operation_store_command, sidebar_locations_command};
use crate::config::UserConfig;
use crate::model::{LoadedOperationStore, Message, SidebarLocation, StartupEnvironment};
use crate::sidebar::home_sidebar_location;
use crate::startup_trace;

impl FileBrowser {
    pub(super) fn accept_startup_environment(
        &mut self,
        startup_environment: StartupEnvironment,
    ) -> Task<Message> {
        startup_trace::mark_once("startup_environment_loaded");
        let home = startup_environment.home;
        let state_database_path = startup_environment.state_database_path;

        self.apply_loaded_user_config(startup_environment.user_config);
        self.current_dir = home.clone();
        self.is_trash_view = false;
        self.path_input = path_text(&self.current_dir);
        self.sidebar_locations = vec![home_sidebar_location(&home)];
        self.entries.clear();
        self.trash_entries.clear();
        self.is_loading = true;
        self.error = None;
        self.sync_active_tab_state();

        Task::batch([
            load_directory_command(home.clone(), self.options.clone()),
            sidebar_locations_command(home),
            operation_store_command(state_database_path),
        ])
    }

    pub(super) fn accept_sidebar_locations(
        &mut self,
        sidebar_locations: Vec<SidebarLocation>,
    ) -> Task<Message> {
        self.sidebar_locations = sidebar_locations;
        Task::none()
    }

    pub(super) fn accept_operation_store(
        &mut self,
        operation_store: Result<LoadedOperationStore, String>,
    ) -> Task<Message> {
        match operation_store {
            Ok(loaded_store) => {
                let persisted_column_width_override = loaded_store.column_width_override;
                self.operation_queue
                    .set_store(loaded_store.task_queue_store);
                if let Some(width) = persisted_column_width_override {
                    self.apply_column_width_override(Some(width));
                    if self
                        .user_config
                        .legacy_column_width_override
                        .take()
                        .is_some()
                    {
                        return self.persist_user_config_command();
                    }
                } else if self.user_config.legacy_column_width_override.is_some() {
                    self.apply_column_width_override(self.user_config.legacy_column_width_override);
                    self.user_config.legacy_column_width_override = None;
                    return Task::batch([
                        self.persist_column_width_override_command(),
                        self.persist_user_config_command(),
                    ]);
                }
            }
            Err(error) => {
                self.error = Some(format!(
                    "Failed to initialize file operation queue storage: {error}"
                ));
            }
        }
        Task::none()
    }

    pub(super) fn accept_thumbnail_refresh_request(&mut self, directory: PathBuf) -> Task<Message> {
        if self.is_loading || self.current_dir != directory {
            return Task::none();
        }
        self.schedule_thumbnail_refresh()
    }

    fn apply_loaded_user_config(&mut self, user_config: UserConfig) {
        self.search_index.base_dir = user_config.search_index_dir.clone();
        self.thumbnail_cache
            .set_cache_dir(user_config.thumbnail_cache_dir.clone());
        self.apply_column_width_override(user_config.legacy_column_width_override);
        self.terminal_emulator = user_config.terminal_emulator;
        self.rendering_gpu_preference = user_config.rendering_gpu_preference;
        self.options.include_hidden = user_config.show_hidden_files;
        self.user_config = user_config;
    }
}
