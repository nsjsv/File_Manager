use iced::Task;

use super::FileBrowser;
use crate::model::Message;

impl FileBrowser {
    pub(super) fn accept_user_preferences_saved(
        &mut self,
        result: Result<(), String>,
    ) -> Task<Message> {
        self.accept_persistence_result(result, "Failed to save user preferences")
    }

    pub(super) fn accept_app_config_saved(&mut self, result: Result<(), String>) -> Task<Message> {
        self.accept_persistence_result(result, "Failed to save application configuration")
    }

    pub(super) fn accept_column_width_saved(
        &mut self,
        result: Result<(), String>,
    ) -> Task<Message> {
        self.accept_persistence_result(result, "Failed to save column width")
    }

    pub(super) fn accept_browser_session_saved(
        &mut self,
        result: Result<(), String>,
    ) -> Task<Message> {
        self.accept_persistence_result(result, "Failed to save browser session")
    }

    fn accept_persistence_result(
        &mut self,
        result: Result<(), String>,
        action: &'static str,
    ) -> Task<Message> {
        if let Err(error) = result {
            self.error = Some(format!("{action}: {error}"));
        }
        Task::none()
    }
}
