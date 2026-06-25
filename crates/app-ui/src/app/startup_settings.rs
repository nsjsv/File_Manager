use std::path::PathBuf;

use iced::Task;

use super::FileBrowser;
use crate::config::StartupLocationPolicy;
use crate::model::Message;

impl FileBrowser {
    pub(super) fn select_startup_location_policy(
        &mut self,
        policy: StartupLocationPolicy,
    ) -> Task<Message> {
        if self.user_config.startup_location_policy == policy {
            return Task::none();
        }
        self.user_config.startup_location_policy = policy;
        Task::batch([
            self.persist_user_preferences_command(),
            self.request_browser_session_save(),
        ])
    }

    pub(super) fn update_startup_custom_directory_input(&mut self, value: String) -> Task<Message> {
        self.startup_custom_directory_input = value;
        self.startup_custom_directory_error = None;
        Task::none()
    }

    pub(super) fn commit_startup_custom_directory_input(&mut self) -> Task<Message> {
        let trimmed = self.startup_custom_directory_input.trim();
        if trimmed.is_empty() {
            self.startup_custom_directory_error = Some("Enter a directory path.".to_owned());
            return Task::none();
        }
        let directory = PathBuf::from(trimmed);
        if !std::fs::metadata(&directory).is_ok_and(|metadata| metadata.is_dir()) {
            self.startup_custom_directory_error = Some("Choose an existing directory.".to_owned());
            return Task::none();
        }
        self.user_config.startup_custom_directory = directory;
        self.startup_custom_directory_input = self
            .user_config
            .startup_custom_directory
            .to_string_lossy()
            .into_owned();
        self.startup_custom_directory_error = None;
        self.persist_user_preferences_command()
    }

    pub(super) fn toggle_save_view_state(&mut self) -> Task<Message> {
        self.user_config.save_view_state = !self.user_config.save_view_state;
        let persist_config = self.persist_user_preferences_command();
        let persist_session = self.request_browser_session_save();
        Task::batch([persist_config, persist_session])
    }
}
