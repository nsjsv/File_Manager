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
        let save_view_state = policy.saves_view_state();
        if self.user_config.startup_location_policy == policy
            && self.user_config.save_view_state == save_view_state
        {
            return Task::none();
        }
        self.user_config.startup_location_policy = policy;
        self.user_config.save_view_state = save_view_state;
        if !save_view_state {
            self.pending_browser_session_save = false;
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    #[test]
    fn previous_state_startup_policy_enables_view_state_saving() {
        let (mut browser, _) = FileBrowser::new(config::ui_thread_startup_config());

        drop(browser.select_startup_location_policy(StartupLocationPolicy::PreviousSession));

        assert_eq!(
            browser.user_config().startup_location_policy,
            StartupLocationPolicy::PreviousSession
        );
        assert!(browser.user_config().save_view_state);
        assert!(browser.pending_browser_session_save);
    }

    #[test]
    fn non_previous_startup_policy_disables_view_state_saving() {
        let mut user_config = config::ui_thread_startup_config();
        user_config.startup_location_policy = StartupLocationPolicy::PreviousSession;
        user_config.save_view_state = user_config.startup_location_policy.saves_view_state();
        let (mut browser, _) = FileBrowser::new(user_config);

        drop(browser.select_startup_location_policy(StartupLocationPolicy::Home));

        assert_eq!(
            browser.user_config().startup_location_policy,
            StartupLocationPolicy::Home
        );
        assert!(!browser.user_config().save_view_state);
        assert!(!browser.pending_browser_session_save);
    }
}
