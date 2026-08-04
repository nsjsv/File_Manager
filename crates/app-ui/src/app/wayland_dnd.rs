use std::path::PathBuf;
use std::sync::Arc;

use desktop_linux::{WaylandDndController, WaylandDndWindowHandle, WaylandFileDragSessionId};
use file_core::FileKind;
use iced::Task;

use super::FileBrowser;
use crate::icons::{file_entry_icon_symbol, IconSymbol};
use crate::model::Message;
use crate::wayland_drag_icon::render_wayland_file_drag_icon;

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

pub(crate) enum WaylandFileDragRequest {
    Unavailable,
    Requested(WaylandFileDragSessionId),
    Rejected(String),
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
            Err(error) => self.show_global_error(error),
        }
        Task::none()
    }

    pub(crate) fn request_wayland_file_drag(&self, paths: Vec<PathBuf>) -> WaylandFileDragRequest {
        if self.is_trash_view || paths.is_empty() {
            return WaylandFileDragRequest::Unavailable;
        }
        let Some(runtime) = &self.wayland_dnd else {
            tracing::debug!(
                path_count = paths.len(),
                "Wayland file drag skipped because no handle is available"
            );
            return WaylandFileDragRequest::Unavailable;
        };
        let path_count = paths.len();
        let first_path = &paths[0];
        let symbol = self
            .entry_for_path(first_path)
            .map(|entry| {
                if entry.kind == FileKind::Symlink && entry.is_broken_symlink {
                    IconSymbol::TriangleAlert
                } else {
                    file_entry_icon_symbol(entry.kind, entry.name())
                }
            })
            .unwrap_or_else(|| {
                file_entry_icon_symbol(
                    FileKind::Other,
                    first_path.file_name().unwrap_or(first_path.as_os_str()),
                )
            });
        let icon = match render_wayland_file_drag_icon(symbol, path_count) {
            Ok(icon) => icon,
            Err(error) => {
                tracing::warn!(%error, path_count, "Wayland file drag icon rendering failed");
                return WaylandFileDragRequest::Rejected(format!(
                    "Could not create Wayland file drag feedback: {error}"
                ));
            }
        };

        match runtime.controller.start_file_drag(paths, icon) {
            Ok(session_id) => {
                tracing::debug!(%session_id, path_count, "Wayland file drag request sent");
                WaylandFileDragRequest::Requested(session_id)
            }
            Err(error) => {
                tracing::warn!(%error, path_count, "Wayland file drag request failed");
                WaylandFileDragRequest::Rejected(format!(
                    "Could not start Wayland file drag: {error}"
                ))
            }
        }
    }

    pub(in crate::app) fn accept_wayland_dnd_runtime_failure(
        &mut self,
        error: String,
    ) -> Task<Message> {
        self.wayland_dnd = None;
        self.cancel_file_drag_interaction();
        self.show_global_error(error);
        Task::none()
    }
}
