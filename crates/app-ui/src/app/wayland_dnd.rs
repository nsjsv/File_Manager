use std::path::PathBuf;
use std::sync::Arc;

use desktop_linux::{WaylandDndController, WaylandDndWindowHandle};
use iced::Task;

use super::FileBrowser;
use crate::model::Message;

#[derive(Clone, Debug)]
pub(super) struct WaylandDndRuntime {
    pub(super) window_handle: WaylandDndWindowHandle,
    pub(super) controller: Arc<WaylandDndController>,
}

impl WaylandDndRuntime {
    fn new(window_handle: WaylandDndWindowHandle) -> Self {
        Self {
            window_handle,
            controller: WaylandDndController::new(),
        }
    }
}

impl FileBrowser {
    pub(in crate::app) fn accept_wayland_dnd_handle(
        &mut self,
        handle: Result<Option<WaylandDndWindowHandle>, String>,
    ) -> Task<Message> {
        match handle {
            Ok(Some(handle)) => {
                tracing::debug!("Wayland drag-and-drop handle loaded");
                self.wayland_dnd = Some(WaylandDndRuntime::new(handle));
            }
            Ok(None) => {
                tracing::debug!("Wayland drag-and-drop unavailable for this window backend");
                self.wayland_dnd = None;
            }
            Err(error) => self.error = Some(error),
        }
        Task::none()
    }

    pub(crate) fn request_wayland_file_drag(&self, paths: Vec<PathBuf>) {
        if self.is_trash_view || paths.is_empty() {
            return;
        }
        let Some(runtime) = &self.wayland_dnd else {
            tracing::debug!(
                path_count = paths.len(),
                "Wayland file drag skipped because no handle is available"
            );
            return;
        };
        let path_count = paths.len();
        if let Err(error) = runtime.controller.start_file_drag(paths) {
            tracing::warn!(%error, path_count, "Wayland file drag request failed");
        } else {
            tracing::debug!(path_count, "Wayland file drag request sent");
        }
    }
}

#[cfg(test)]
mod tests {
    use desktop_linux::WaylandDndWindowHandle;

    use super::*;
    use crate::config;

    #[test]
    fn accepts_wayland_dnd_window_handle() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let handle = WaylandDndWindowHandle::new(1, 2);

        let command = browser.accept_wayland_dnd_handle(Ok(Some(handle)));
        drop(command);

        assert_eq!(
            browser
                .wayland_dnd
                .as_ref()
                .map(|runtime| runtime.window_handle),
            Some(handle)
        );
        assert!(browser.error.is_none());
    }

    #[test]
    fn wayland_dnd_window_handle_error_is_visible() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        let command = browser.accept_wayland_dnd_handle(Err("no wayland handle".to_owned()));
        drop(command);

        assert_eq!(browser.error.as_deref(), Some("no wayland handle"));
    }
}
