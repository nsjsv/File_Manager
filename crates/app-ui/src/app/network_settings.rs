use iced::Task;

use super::FileBrowser;
use crate::app::remote_mounts::path_is_remote_mount;
use crate::config;
use crate::model::Message;
use crate::thumbnail_cache::{ThumbnailLoadPolicy, ThumbnailPurpose};

impl FileBrowser {
    pub(super) fn toggle_network_list_thumbnail_downloads(&mut self) -> Task<Message> {
        self.user_config.network_list_thumbnail_downloads_enabled =
            !self.user_config.network_list_thumbnail_downloads_enabled;
        if !self.user_config.network_list_thumbnail_downloads_enabled {
            self.remove_pending_network_thumbnail_generations();
        }

        Task::batch([
            self.persist_user_preferences_command(),
            self.schedule_thumbnail_refresh(),
        ])
    }

    pub(super) fn update_max_preview_file_mib_input(&mut self, value: String) -> Task<Message> {
        self.max_preview_file_mib_input = value;
        self.max_preview_file_mib_error = None;
        Task::none()
    }

    pub(super) fn commit_max_preview_file_mib_input(&mut self) -> Task<Message> {
        let trimmed = self.max_preview_file_mib_input.trim();
        let Ok(mib) = trimmed.parse::<u64>() else {
            self.max_preview_file_mib_error =
                Some("Enter a whole number of MiB greater than 0.".to_owned());
            return Task::none();
        };
        if mib == 0 {
            self.max_preview_file_mib_error =
                Some("Enter a whole number of MiB greater than 0.".to_owned());
            return Task::none();
        }
        let Some(bytes) = config::max_preview_file_bytes_from_mib(mib) else {
            self.max_preview_file_mib_error = Some("Enter a smaller preview size.".to_owned());
            return Task::none();
        };

        self.max_preview_file_mib_input = config::max_preview_file_mib(bytes).to_string();
        self.max_preview_file_mib_error = None;
        if self.user_config.max_preview_file_bytes == bytes {
            return Task::none();
        }

        self.user_config.max_preview_file_bytes = bytes;
        self.persist_user_preferences_command()
    }

    fn remove_pending_network_thumbnail_generations(&mut self) {
        let network_connections = self.network_connections.clone();
        let sidebar_devices = self.sidebar_devices.clone();
        self.thumbnail_cache.retain_queued_work(|work| {
            work.purpose == ThumbnailPurpose::Preview
                || work.load_policy == ThumbnailLoadPolicy::CacheOnly
                || !path_is_remote_mount(
                    &network_connections,
                    &sidebar_devices,
                    &work.request.source,
                )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committing_zero_preview_size_keeps_existing_config_and_reports_error() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let original_max_preview_file_bytes = browser.user_config.max_preview_file_bytes;

        let _ = browser.update_max_preview_file_mib_input("0".to_owned());
        let _ = browser.commit_max_preview_file_mib_input();

        assert_eq!(
            browser.user_config.max_preview_file_bytes,
            original_max_preview_file_bytes
        );
        assert_eq!(
            browser.max_preview_file_mib_error.as_deref(),
            Some("Enter a whole number of MiB greater than 0.")
        );
    }
}
