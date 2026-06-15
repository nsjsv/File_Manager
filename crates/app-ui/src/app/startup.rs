use std::path::{Path, PathBuf};

use iced::Task;

use super::paths::path_text;
use super::FileBrowser;
use crate::commands::{
    load_directory_command, operation_store_command, sidebar_devices_command,
    sidebar_locations_command,
};
use crate::config::UserConfig;
use crate::model::{
    BrowserPaneId, LoadedOperationStore, Message, SidebarLocation, StartupEnvironment,
};
use crate::operation_queue::QueuedFileOperation;
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
        let configured_favorites = self.user_config.sidebar_favorites.clone();
        self.current_dir = home.clone();
        self.is_trash_view = false;
        self.path_input = path_text(&self.current_dir);
        self.sidebar_locations = vec![home_sidebar_location(&home)];
        self.entries.clear();
        self.directory_loading_placeholder_entries.clear();
        self.trash_entries.clear();
        self.deepest_open_column_directory = None;
        self.is_loading = true;
        self.error = None;
        self.sync_active_tab_state();
        let startup_index_setup_command = self.refresh_startup_index_setup_choices();

        Task::batch([
            startup_index_setup_command,
            load_directory_command(BrowserPaneId::PRIMARY, home.clone(), self.options.clone()),
            sidebar_locations_command(home, configured_favorites),
            sidebar_devices_command(),
            operation_store_command(state_database_path),
        ])
    }

    pub(super) fn accept_sidebar_locations(
        &mut self,
        sidebar_locations: Vec<SidebarLocation>,
    ) -> Task<Message> {
        self.sidebar_locations = sidebar_locations;
        self.refresh_startup_index_setup_choices()
    }

    pub(super) fn accept_operation_store(
        &mut self,
        operation_store: Result<LoadedOperationStore, String>,
    ) -> Task<Message> {
        match operation_store {
            Ok(loaded_store) => {
                let persisted_column_width_overrides = loaded_store.column_width_overrides;
                let previous_task_count = self.operation_queue.task_count();
                if let Some(error) = self.operation_queue.set_store_and_restore(
                    loaded_store.task_queue_store,
                    loaded_store.restored_tasks,
                ) {
                    self.error = Some(error);
                }
                for task in self.operation_queue.tasks() {
                    if let QueuedFileOperation::BuildSearchIndex { root, .. } = &task.operation {
                        self.search_index.indexing_roots.insert(root.clone());
                        self.search_index.errors.remove(root);
                    }
                }
                let restored_queue_command =
                    if self.operation_queue.task_count() > previous_task_count {
                        self.show_operation_queue_temporarily()
                    } else {
                        Task::none()
                    };
                if !persisted_column_width_overrides.is_empty() {
                    self.apply_column_width_overrides(persisted_column_width_overrides);
                    if !self.user_config.legacy_column_width_overrides.is_empty() {
                        self.user_config.legacy_column_width_overrides.clear();
                        return Task::batch([
                            self.persist_user_config_command(),
                            restored_queue_command,
                        ]);
                    }
                } else if !self.user_config.legacy_column_width_overrides.is_empty() {
                    self.apply_column_width_overrides(
                        self.user_config.legacy_column_width_overrides.clone(),
                    );
                    self.user_config.legacy_column_width_overrides.clear();
                    return Task::batch([
                        self.persist_column_width_overrides_command(),
                        self.persist_user_config_command(),
                        restored_queue_command,
                    ]);
                }
                return restored_queue_command;
            }
            Err(error) => {
                self.error = Some(format!(
                    "Failed to initialize file operation queue storage: {error}"
                ));
            }
        }
        Task::none()
    }

    pub(super) fn accept_thumbnail_refresh_request(
        &mut self,
        pane_id: BrowserPaneId,
        directory: PathBuf,
    ) -> Task<Message> {
        if !self.thumbnail_refresh_matches_pane(pane_id, &directory) {
            return Task::none();
        }
        self.schedule_thumbnail_refresh_for_pane(pane_id)
    }

    fn thumbnail_refresh_matches_pane(&self, pane_id: BrowserPaneId, directory: &Path) -> bool {
        if pane_id == self.active_pane_id() {
            return !self.is_loading && self.current_dir.as_path() == directory;
        }

        self.pane_by_id(pane_id)
            .is_some_and(|pane| !pane.is_loading && pane.current_dir.as_path() == directory)
    }

    fn apply_loaded_user_config(&mut self, user_config: UserConfig) {
        self.search_index.base_dir = user_config.search_index_dir.clone();
        self.thumbnail_cache
            .set_cache_dir(user_config.thumbnail_cache_dir.clone());
        self.apply_column_width_overrides(user_config.legacy_column_width_overrides.clone());
        self.sidebar_width = self.sidebar_width_for_window(user_config.sidebar_width);
        self.terminal_emulator = user_config.terminal_emulator;
        self.rendering_gpu_preference = user_config.rendering_gpu_preference;
        self.options.include_hidden = user_config.show_hidden_files;
        self.view_mode = user_config.browser_view_mode;
        self.user_config = user_config;
    }
}
