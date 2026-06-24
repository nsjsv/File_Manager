use std::time::{Duration, Instant};

use iced::Task;

use super::FileBrowser;
use crate::commands::save_browser_session_command;
use crate::model::{
    pane_session_from_live, search_session_from_live, BrowserSessionSnapshot,
    FilePropertiesLoadState, FilePropertiesState, Message, PreviewContent, PreviewState,
};

const SESSION_SAVE_INTERVAL: Duration = Duration::from_millis(500);

impl FileBrowser {
    pub(super) fn browser_session_snapshot(&mut self) -> BrowserSessionSnapshot {
        self.sync_active_tab_state();
        let panes = self
            .panes
            .iter()
            .map(pane_session_from_live)
            .collect::<Vec<_>>();
        BrowserSessionSnapshot {
            panes,
            layout: self.pane_layout,
            search: self.search.as_ref().map(search_session_from_live),
            preview_path: active_preview_path(self.preview.as_ref()),
            properties: self.properties.as_ref().map(properties_session_from_live),
            settings_category: self
                .settings_window
                .map(|_| self.selected_settings_category),
        }
    }

    pub(super) fn request_browser_session_save(&mut self) -> Task<Message> {
        if !self.user_config.save_view_state {
            return Task::none();
        }
        let Some(store) = self.operation_queue.task_queue_store().cloned() else {
            self.pending_browser_session_save = true;
            return Task::none();
        };
        let now = Instant::now();
        if self
            .last_browser_session_save
            .is_some_and(|last| now.duration_since(last) < SESSION_SAVE_INTERVAL)
        {
            self.pending_browser_session_save = true;
            return browser_session_save_delay_command();
        }
        self.last_browser_session_save = Some(now);
        self.pending_browser_session_save = false;
        save_browser_session_command(store, self.browser_session_snapshot())
    }

    pub(super) fn flush_browser_session_save(&mut self) -> Task<Message> {
        if !self.user_config.save_view_state {
            return Task::none();
        }
        let Some(store) = self.operation_queue.task_queue_store().cloned() else {
            return Task::none();
        };
        self.last_browser_session_save = Some(Instant::now());
        self.pending_browser_session_save = false;
        save_browser_session_command(store, self.browser_session_snapshot())
    }

    pub(super) fn maybe_flush_pending_browser_session_save(&mut self) -> Task<Message> {
        if !self.pending_browser_session_save {
            return Task::none();
        }
        let Some(last) = self.last_browser_session_save else {
            return self.request_browser_session_save();
        };
        if last.elapsed() >= SESSION_SAVE_INTERVAL {
            return self.flush_browser_session_save();
        }
        Task::none()
    }
}

fn browser_session_save_delay_command() -> Task<Message> {
    Task::perform(
        async {
            tokio::time::sleep(SESSION_SAVE_INTERVAL).await;
        },
        |_| Message::BrowserSessionSaveDelayElapsed,
    )
}

fn active_preview_path(preview: Option<&PreviewState>) -> Option<std::path::PathBuf> {
    match preview? {
        PreviewState::Loading(path) => Some(path.clone()),
        PreviewState::DownloadingNetworkFile(download) => Some(download.source_path.clone()),
        PreviewState::Ready(content) => preview_content_path(content),
        PreviewState::Error(_) => None,
    }
}

fn preview_content_path(content: &PreviewContent) -> Option<std::path::PathBuf> {
    match content {
        PreviewContent::Text { path, .. }
        | PreviewContent::Image { path, .. }
        | PreviewContent::Audio { path, .. }
        | PreviewContent::Video { path, .. } => Some(path.clone()),
        PreviewContent::AnimatedImage(preview) => Some(preview.path().to_path_buf()),
        PreviewContent::Directory { .. } | PreviewContent::Archive { .. } => None,
    }
}

fn properties_session_from_live(
    properties: &FilePropertiesState,
) -> crate::model::PropertiesSessionSnapshot {
    let category = properties.selected_category;
    let path = match &properties.load_state {
        FilePropertiesLoadState::Loading
        | FilePropertiesLoadState::Loaded(_)
        | FilePropertiesLoadState::Failed(_) => properties.path.clone(),
    };
    crate::model::PropertiesSessionSnapshot { path, category }
}
