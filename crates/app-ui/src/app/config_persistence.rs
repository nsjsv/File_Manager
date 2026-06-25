use iced::Task;

use super::FileBrowser;
use crate::commands::{save_app_config_command, save_user_preferences_command};
use crate::config;
use crate::model::Message;

impl FileBrowser {
    pub(super) fn persist_user_preferences_command(&self) -> Task<Message> {
        save_user_preferences_command(
            self.user_config.user_preferences(),
            self.operation_queue.task_queue_store().cloned(),
        )
    }

    pub(super) fn persist_app_config_command(&self) -> Task<Message> {
        save_app_config_command(config::AppConfig::from_user_config(&self.user_config))
    }
}
