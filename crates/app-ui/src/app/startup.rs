use iced::Command;

use super::paths::path_text;
use super::FileBrowser;
use crate::model::{InitialLoad, Message};
use crate::startup_trace;

impl FileBrowser {
    pub(super) fn accept_initial_load(&mut self, initial_load: InitialLoad) -> Command<Message> {
        startup_trace::mark_once("initial_directory_loaded_message");
        self.sidebar_locations = initial_load.sidebar_locations;
        let user_config = initial_load.user_config;
        self.search_index.base_dir = user_config.search_index_dir.clone();
        self.thumbnail_cache
            .set_cache_dir(user_config.thumbnail_cache_dir.clone());
        self.column_view_mode = user_config.column_view_mode;
        self.column_fixed_count = user_config.column_fixed_count;
        self.unbounded_column_width = user_config.unbounded_column_width;
        self.terminal_emulator = user_config.terminal_emulator;
        self.rendering_backend_preference = user_config.rendering_backend_preference;
        self.options.include_hidden = user_config.show_hidden_files;
        self.user_config = user_config;
        let mut operation_store_error = match initial_load.operation_store {
            Ok(store) => {
                self.operation_queue.set_store(store);
                None
            }
            Err(error) => Some(format!(
                "Failed to initialize file operation queue storage: {error}"
            )),
        };

        let command = match initial_load.scan {
            Ok(scan) => {
                self.current_dir = scan.path;
                self.is_trash_view = false;
                self.path_input = path_text(&self.current_dir);
                self.entries = scan.entries;
                self.trash_entries.clear();
                self.is_loading = false;
                self.error = operation_store_error.take();
                startup_trace::mark_once("initial_directory_ready");
                self.schedule_thumbnail_refresh()
            }
            Err(error) => {
                self.current_dir = initial_load.home;
                self.is_trash_view = false;
                self.path_input = path_text(&self.current_dir);
                self.entries.clear();
                self.trash_entries.clear();
                self.is_loading = false;
                self.error = Some(match operation_store_error.take() {
                    Some(storage_error) => format!("{error}; {storage_error}"),
                    None => error,
                });
                Command::none()
            }
        };

        self.sync_active_tab_state();
        command
    }
}
