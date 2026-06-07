use iced::Task;

use super::FileBrowser;
use crate::config::RenderingGpuPreference;
use crate::model::Message;

impl FileBrowser {
    pub(super) fn select_rendering_gpu_preference(
        &mut self,
        preference: RenderingGpuPreference,
    ) -> Task<Message> {
        if self.rendering_gpu_preference == preference {
            return Task::none();
        }

        self.rendering_gpu_preference = preference;
        self.user_config.rendering_gpu_preference = preference;
        self.renderer_restart_notice_visible = true;
        self.is_column_view_settings_open = false;
        self.persist_user_config_command()
    }

    pub(super) fn dismiss_renderer_restart_notice(&mut self) -> Task<Message> {
        self.renderer_restart_notice_visible = false;
        Task::none()
    }
}
