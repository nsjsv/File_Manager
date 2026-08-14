use iced::Task;

use super::FileBrowser;
use crate::commands::{save_app_config_command, save_user_preferences_command};
use crate::config;
use crate::model::Message;

impl FileBrowser {
    pub(super) fn persist_user_preferences_command(&mut self) -> Task<Message> {
        let preferences = self.user_config.user_preferences();
        if self.user_preferences_save_in_flight {
            self.pending_user_preferences_save = Some(preferences);
            return Task::none();
        }

        self.user_preferences_save_in_flight = true;
        self.save_user_preferences_snapshot_command(preferences)
    }

    pub(super) fn continue_user_preferences_save(&mut self) -> Task<Message> {
        let Some(preferences) = self.pending_user_preferences_save.take() else {
            self.user_preferences_save_in_flight = false;
            return Task::none();
        };
        self.save_user_preferences_snapshot_command(preferences)
    }

    fn save_user_preferences_snapshot_command(
        &self,
        preferences: config::UserPreferences,
    ) -> Task<Message> {
        save_user_preferences_command(
            preferences,
            self.operation_queue.task_queue_store().cloned(),
        )
    }

    pub(super) fn persist_app_config_command(&self) -> Task<Message> {
        save_app_config_command(config::AppConfig::from_user_config(&self.user_config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_preferences_save_coalesces_to_the_latest_pending_snapshot() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        drop(browser.persist_user_preferences_command());
        assert!(browser.user_preferences_save_in_flight);
        assert!(browser.pending_user_preferences_save.is_none());

        browser.user_config.show_hidden_files = true;
        drop(browser.persist_user_preferences_command());
        browser.user_config.sidebar_width = 240.0;
        drop(browser.persist_user_preferences_command());

        let pending = browser
            .pending_user_preferences_save
            .as_ref()
            .expect("latest preferences snapshot");
        assert!(pending.show_hidden_files);
        assert_eq!(pending.sidebar_width, 240.0);

        drop(browser.accept_user_preferences_saved(Ok(())));
        assert!(browser.user_preferences_save_in_flight);
        assert!(browser.pending_user_preferences_save.is_none());

        drop(browser.accept_user_preferences_saved(Ok(())));
        assert!(!browser.user_preferences_save_in_flight);
    }

    #[test]
    fn failed_user_preferences_save_still_advances_to_the_latest_snapshot() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        drop(browser.persist_user_preferences_command());
        browser.user_config.show_hidden_files = true;
        drop(browser.persist_user_preferences_command());

        drop(browser.accept_user_preferences_saved(Err("read-only".to_owned())));

        assert!(browser.user_preferences_save_in_flight);
        assert!(browser.pending_user_preferences_save.is_none());
        assert!(browser
            .error
            .as_deref()
            .is_some_and(|error| error.contains("read-only")));
    }
}
