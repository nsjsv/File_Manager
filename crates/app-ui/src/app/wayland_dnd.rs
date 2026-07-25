use std::path::PathBuf;
use std::sync::Arc;

use desktop_linux::{
    WaylandDndController, WaylandDndWindowHandle, WaylandFileDragSelfTargetEvent,
    WaylandFileDragSessionId, WaylandFileDragSourceEvent,
};
use file_core::FileKind;
use iced::{Point, Task};

use super::FileBrowser;
use crate::icons::{file_entry_icon_symbol, IconSymbol};
use crate::model::{FileDragNativeDndState, Message};
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

    pub(in crate::app) fn accept_wayland_file_drag_source_event(
        &mut self,
        event: WaylandFileDragSourceEvent,
    ) -> Task<Message> {
        let session_id = event.session_id();
        let active_session_id = self
            .file_drag
            .as_ref()
            .and_then(|file_drag| file_drag.native_dnd.session_id());
        if active_session_id != Some(session_id) {
            tracing::debug!(
                %session_id,
                "Ignoring stale Wayland file drag source event"
            );
            return Task::none();
        }

        match event {
            WaylandFileDragSourceEvent::Started(session_id) => {
                if let Some(file_drag) = &mut self.file_drag {
                    if file_drag.native_dnd == FileDragNativeDndState::Requested(session_id) {
                        file_drag.native_dnd = FileDragNativeDndState::Started(session_id);
                    }
                }
            }
            WaylandFileDragSourceEvent::Dropped(session_id) => {
                if let Some(file_drag) = &mut self.file_drag {
                    if matches!(
                        file_drag.native_dnd,
                        FileDragNativeDndState::Requested(active_id)
                            | FileDragNativeDndState::Started(active_id)
                            if active_id == session_id
                    ) {
                        file_drag.native_dnd = FileDragNativeDndState::Dropped(session_id);
                    }
                }
                self.clear_wayland_file_drag_highlight();
            }
            WaylandFileDragSourceEvent::Finished(session_id)
            | WaylandFileDragSourceEvent::Cancelled(session_id) => {
                self.clear_wayland_file_drag_session(session_id);
            }
            WaylandFileDragSourceEvent::Rejected {
                session_id,
                details,
            } => {
                self.clear_wayland_file_drag_session(session_id);
                self.show_global_error(format!("Could not start Wayland file drag: {details}"));
            }
        }
        Task::none()
    }

    pub(in crate::app) fn accept_wayland_file_drag_self_target_event(
        &mut self,
        event: WaylandFileDragSelfTargetEvent,
    ) -> Task<Message> {
        let session_id = event.session_id();
        let active_session_id = self
            .file_drag
            .as_ref()
            .and_then(|file_drag| file_drag.native_dnd.session_id());
        if active_session_id != Some(session_id) {
            tracing::debug!(
                %session_id,
                "Ignoring stale Wayland file drag self-target event"
            );
            return Task::none();
        }

        match event {
            WaylandFileDragSelfTargetEvent::Entered { position, .. }
            | WaylandFileDragSelfTargetEvent::Moved { position, .. } => {
                let position = Point::new(position.x as f32, position.y as f32);
                self.cursor_position = position;
                self.refresh_wayland_file_drag_target_at_position(session_id, position);
            }
            WaylandFileDragSelfTargetEvent::Left { .. } => {
                self.clear_wayland_file_drag_target();
            }
        }
        Task::none()
    }

    pub(in crate::app) fn accept_wayland_dnd_runtime_failure(
        &mut self,
        error: String,
    ) -> Task<Message> {
        if let Some(session_id) = self
            .file_drag
            .as_ref()
            .and_then(|file_drag| file_drag.native_dnd.session_id())
        {
            self.clear_wayland_file_drag_session(session_id);
        }
        self.accept_wayland_file_drop(Err(error))
    }

    fn clear_wayland_file_drag_session(&mut self, session_id: WaylandFileDragSessionId) {
        let active_session_id = self
            .file_drag
            .as_ref()
            .and_then(|file_drag| file_drag.native_dnd.session_id());
        if active_session_id != Some(session_id) {
            return;
        }
        self.clear_wayland_file_drag_target();
        self.file_drag = None;
        self.drag_selection_anchor = None;
        self.sidebar_bookmark_drop_slot = None;
    }
}

#[cfg(test)]
mod tests {
    use desktop_linux::{WaylandDndDropPosition, WaylandDndWindowHandle, WaylandFileDragIcon};
    use iced::{Rectangle, Size};

    use super::*;
    use crate::config;
    use crate::model::{
        BreadcrumbDropTargetBounds, FileDragPhase, FileDragState, FileDragTarget,
        SidebarBookmarkDropSlot,
    };

    fn file_drag_session_id() -> WaylandFileDragSessionId {
        WaylandDndController::new()
            .start_file_drag(
                vec![PathBuf::from("/workspace/report.txt")],
                WaylandFileDragIcon::new(1, 1, vec![0, 0, 0, 0]).unwrap(),
            )
            .unwrap()
    }

    fn browser_with_native_file_drag(native_dnd: FileDragNativeDndState) -> FileBrowser {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let source = PathBuf::from("/workspace/report.txt");
        browser.file_drag = Some(FileDragState {
            sources: vec![source.clone()],
            pressed_path: source.clone(),
            stationary_action: crate::model::FileDragStationaryAction::SelectionOnly,
            target: None,
            phase: FileDragPhase::Dragging,
            native_dnd,
            wayland_target: native_dnd.session_id().map(|session_id| {
                crate::model::WaylandFileDragTargetSnapshot {
                    session_id,
                    hit_test_bounds: crate::model::WaylandFileDragHitTestBounds::default(),
                    bookmark_source: None,
                    position: None,
                    target: None,
                }
            }),
            column_directories_snapshot: Vec::new(),
        });
        browser.drag_selection_anchor = Some(source);
        browser.sidebar_bookmark_drop_slot = Some(SidebarBookmarkDropSlot::Insert { index: 1 });
        browser
    }

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

    #[test]
    fn started_source_session_hands_preview_to_wayland() {
        let session_id = file_drag_session_id();
        let mut browser =
            browser_with_native_file_drag(FileDragNativeDndState::Requested(session_id));
        assert!(browser
            .file_drag
            .as_ref()
            .is_some_and(FileDragState::displays_iced_drag_preview));

        drop(
            browser.accept_wayland_file_drag_source_event(WaylandFileDragSourceEvent::Started(
                session_id,
            )),
        );

        let file_drag = browser
            .file_drag
            .as_ref()
            .expect("file drag remains active");
        assert_eq!(
            file_drag.native_dnd,
            FileDragNativeDndState::Started(session_id)
        );
        assert!(!file_drag.displays_iced_drag_preview());
    }

    #[test]
    fn iced_release_does_not_consume_native_file_drag_session() {
        let session_id = file_drag_session_id();
        let mut browser =
            browser_with_native_file_drag(FileDragNativeDndState::Started(session_id));

        drop(browser.finish_drag_selection(Some(PathBuf::from("/workspace/target"))));

        assert_eq!(
            browser
                .file_drag
                .as_ref()
                .map(|file_drag| file_drag.native_dnd),
            Some(FileDragNativeDndState::Started(session_id))
        );
    }

    #[test]
    fn matching_self_target_motion_updates_and_leave_clears_directory_target() {
        let session_id = file_drag_session_id();
        let mut browser =
            browser_with_native_file_drag(FileDragNativeDndState::Started(session_id));
        let target_directory = PathBuf::from("/workspace/target");
        let target_bounds = BreadcrumbDropTargetBounds {
            pane_id: browser.active_pane_id(),
            directory: target_directory.clone(),
            item_bounds: Rectangle::new(Point::new(300.0, 10.0), Size::new(100.0, 30.0)),
            viewport_bounds: Rectangle::new(Point::new(280.0, 0.0), Size::new(160.0, 50.0)),
        };
        let pane_id = browser.active_pane_id();
        let snapshot = &mut browser
            .file_drag
            .as_mut()
            .expect("active file drag")
            .wayland_target
            .as_mut()
            .expect("Wayland target snapshot")
            .hit_test_bounds;
        snapshot.breadcrumbs = vec![target_bounds];
        snapshot.directory_targets = vec![crate::model::DirectoryFileDragTargetBounds {
            pane_id,
            directory: PathBuf::from("/workspace"),
            bounds: Rectangle::new(Point::new(280.0, 0.0), Size::new(500.0, 500.0)),
        }];

        drop(browser.accept_wayland_file_drag_self_target_event(
            WaylandFileDragSelfTargetEvent::Moved {
                session_id,
                position: WaylandDndDropPosition { x: 320.0, y: 20.0 },
            },
        ));

        assert!(matches!(
            browser
                .file_drag
                .as_ref()
                .and_then(|file_drag| file_drag.target.as_ref()),
            Some(FileDragTarget::Directory(directory)) if directory == &target_directory
        ));

        drop(browser.accept_wayland_file_drag_self_target_event(
            WaylandFileDragSelfTargetEvent::Left { session_id },
        ));

        assert!(browser
            .file_drag
            .as_ref()
            .is_some_and(|file_drag| file_drag.target.is_none()));
    }

    #[test]
    fn stale_self_target_event_does_not_change_active_target() {
        let stale_session_id = file_drag_session_id();
        let active_session_id = file_drag_session_id();
        let mut browser =
            browser_with_native_file_drag(FileDragNativeDndState::Started(active_session_id));
        let target_directory = PathBuf::from("/workspace/current-target");
        browser.file_drag.as_mut().expect("active file drag").target =
            Some(FileDragTarget::Directory(target_directory.clone()));

        drop(browser.accept_wayland_file_drag_self_target_event(
            WaylandFileDragSelfTargetEvent::Left {
                session_id: stale_session_id,
            },
        ));

        assert!(matches!(
            browser
                .file_drag
                .as_ref()
                .and_then(|file_drag| file_drag.target.as_ref()),
            Some(FileDragTarget::Directory(directory)) if directory == &target_directory
        ));
    }

    #[test]
    fn physical_drop_waits_for_source_terminal_event() {
        let session_id = file_drag_session_id();
        let mut browser =
            browser_with_native_file_drag(FileDragNativeDndState::Started(session_id));
        browser.file_drag.as_mut().expect("active file drag").target = Some(
            FileDragTarget::Directory(PathBuf::from("/workspace/target")),
        );

        drop(
            browser.accept_wayland_file_drag_source_event(WaylandFileDragSourceEvent::Dropped(
                session_id,
            )),
        );

        let file_drag = browser
            .file_drag
            .as_ref()
            .expect("physical drop keeps source payload session");
        assert_eq!(
            file_drag.native_dnd,
            FileDragNativeDndState::Dropped(session_id)
        );
        assert!(file_drag.target.is_none());
        assert!(!file_drag.displays_iced_drag_preview());
    }

    #[test]
    fn source_finish_and_cancel_clear_matching_file_drag_session() {
        let session_id = file_drag_session_id();
        for event in [
            WaylandFileDragSourceEvent::Finished(session_id),
            WaylandFileDragSourceEvent::Cancelled(session_id),
        ] {
            let mut browser =
                browser_with_native_file_drag(FileDragNativeDndState::Started(session_id));

            drop(browser.accept_wayland_file_drag_source_event(event));

            assert!(browser.file_drag.is_none());
            assert!(browser.drag_selection_anchor.is_none());
            assert!(browser.sidebar_bookmark_drop_slot.is_none());
        }
    }

    #[test]
    fn source_rejection_clears_session_and_reports_start_failure() {
        let session_id = file_drag_session_id();
        let mut browser =
            browser_with_native_file_drag(FileDragNativeDndState::Requested(session_id));

        drop(
            browser.accept_wayland_file_drag_source_event(WaylandFileDragSourceEvent::Rejected {
                session_id,
                details: "pointer serial expired".to_owned(),
            }),
        );

        assert!(browser.file_drag.is_none());
        assert_eq!(
            browser.error.as_deref(),
            Some("Could not start Wayland file drag: pointer serial expired")
        );
    }

    #[test]
    fn stale_source_terminal_event_does_not_clear_new_file_drag() {
        let stale_session_id = file_drag_session_id();
        let active_session_id = file_drag_session_id();
        let mut browser =
            browser_with_native_file_drag(FileDragNativeDndState::Started(active_session_id));

        drop(
            browser.accept_wayland_file_drag_source_event(WaylandFileDragSourceEvent::Finished(
                stale_session_id,
            )),
        );

        assert_eq!(
            browser
                .file_drag
                .as_ref()
                .map(|file_drag| file_drag.native_dnd),
            Some(FileDragNativeDndState::Started(active_session_id))
        );
        assert!(browser.drag_selection_anchor.is_some());
        assert!(browser.sidebar_bookmark_drop_slot.is_some());
    }

    #[test]
    fn runtime_failure_clears_native_file_drag_and_reports_error() {
        let session_id = file_drag_session_id();
        let mut browser =
            browser_with_native_file_drag(FileDragNativeDndState::Started(session_id));

        drop(
            browser.accept_wayland_dnd_runtime_failure("Wayland event dispatch stopped".to_owned()),
        );

        assert!(browser.file_drag.is_none());
        assert_eq!(
            browser.error.as_deref(),
            Some("Wayland event dispatch stopped")
        );
    }
}
