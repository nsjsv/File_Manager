use iced::Task;

use super::FileBrowser;
use crate::config::RenderingBackendPreference;
use crate::model::Message;

impl FileBrowser {
    pub(super) fn select_rendering_backend_preference(
        &mut self,
        preference: RenderingBackendPreference,
    ) -> Task<Message> {
        if self.rendering_backend_preference == preference {
            return Task::none();
        }

        self.rendering_backend_preference = preference;
        self.user_config.rendering_backend_preference = preference;
        self.renderer_restart_notice_visible = true;
        self.is_column_view_settings_open = false;
        self.persist_user_config_command()
    }

    pub(super) fn dismiss_renderer_restart_notice(&mut self) -> Task<Message> {
        self.renderer_restart_notice_visible = false;
        Task::none()
    }
}
