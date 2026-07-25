use std::path::PathBuf;

use desktop_linux::{
    WaylandDndDropOrigin, WaylandDndDropPosition, WaylandDndFileDrop, WaylandFileDragSessionId,
};
use iced::{Point, Task};

use super::super::FileBrowser;
use crate::app::PendingWaylandFileDrop;
use crate::breadcrumb_drop_target_bounds::breadcrumb_drop_target_bounds_command;
use crate::model::{BreadcrumbDropTargetBounds, Message};

impl FileBrowser {
    pub(in crate::app) fn accept_wayland_file_drop(
        &mut self,
        drop: Result<WaylandDndFileDrop, String>,
    ) -> Task<Message> {
        match drop {
            Ok(
                drop @ WaylandDndFileDrop {
                    origin: WaylandDndDropOrigin::Internal(session_id),
                    ..
                },
            ) => self.accept_internal_wayland_file_drop(session_id, drop),
            Ok(drop) => self.measure_wayland_file_drop_target(drop),
            Err(error) => {
                self.pending_wayland_file_drop = None;
                self.breadcrumb_drop_target_measurement_generation = self
                    .breadcrumb_drop_target_measurement_generation
                    .wrapping_add(1);
                self.show_global_error(error);
                Task::none()
            }
        }
    }

    fn measure_wayland_file_drop_target(&mut self, drop: WaylandDndFileDrop) -> Task<Message> {
        let generation = self.next_breadcrumb_drop_target_measurement_generation();
        self.pending_wayland_file_drop = Some(PendingWaylandFileDrop {
            measurement_generation: generation,
            drop,
        });
        breadcrumb_drop_target_bounds_command(generation)
    }

    pub(in crate::app) fn request_breadcrumb_drop_target_bounds_measurement(
        &mut self,
    ) -> Task<Message> {
        if self.pending_wayland_file_drop.is_some() {
            return Task::none();
        }
        let generation = self.next_breadcrumb_drop_target_measurement_generation();
        breadcrumb_drop_target_bounds_command(generation)
    }

    pub(in crate::app) fn accept_breadcrumb_drop_target_bounds(
        &mut self,
        generation: u64,
        bounds: Vec<BreadcrumbDropTargetBounds>,
    ) -> Task<Message> {
        if generation != self.breadcrumb_drop_target_measurement_generation {
            return Task::none();
        }

        self.breadcrumb_drop_target_bounds = bounds;
        let pending_generation_matches = self
            .pending_wayland_file_drop
            .as_ref()
            .is_some_and(|pending| pending.measurement_generation == generation);
        if !pending_generation_matches {
            return Task::none();
        }

        let pending = self
            .pending_wayland_file_drop
            .take()
            .expect("matching Wayland file drop");
        match pending.drop.origin {
            WaylandDndDropOrigin::External => self.accept_external_wayland_file_drop(pending.drop),
            WaylandDndDropOrigin::Internal(_) => Task::none(),
        }
    }

    fn next_breadcrumb_drop_target_measurement_generation(&mut self) -> u64 {
        self.breadcrumb_drop_target_measurement_generation = self
            .breadcrumb_drop_target_measurement_generation
            .wrapping_add(1);
        self.breadcrumb_drop_target_measurement_generation
    }

    fn accept_external_wayland_file_drop(&mut self, drop: WaylandDndFileDrop) -> Task<Message> {
        let position = wayland_drop_position(drop.position);
        if let Some(position) = position {
            self.cursor_position = position;
        }
        let paste_directory = position
            .and_then(|position| self.directory_drop_target_at_position(position))
            .or_else(|| match position {
                Some(position) if self.pane_id_at_position(position).is_some() => None,
                Some(_) | None if !self.is_trash_view => Some(self.paste_target_directory()),
                Some(_) | None => None,
            });
        let Some(paste_directory) = paste_directory else {
            return Task::none();
        };
        self.request_file_drop_prompt(paste_directory, drop.selection.paths)
    }

    fn accept_internal_wayland_file_drop(
        &mut self,
        session_id: WaylandFileDragSessionId,
        drop: WaylandDndFileDrop,
    ) -> Task<Message> {
        if !self.active_file_drag_matches_wayland_drop(session_id, &drop.selection.paths) {
            return Task::none();
        }

        let target = wayland_drop_position(drop.position).and_then(|position| {
            self.cursor_position = position;
            self.wayland_file_drag_target_at_drop(session_id, position)
        });

        Task::batch([
            self.finish_wayland_file_drag(session_id, target),
            self.schedule_thumbnail_refresh(),
        ])
    }

    fn active_file_drag_matches_wayland_drop(
        &self,
        session_id: WaylandFileDragSessionId,
        paths: &[PathBuf],
    ) -> bool {
        self.file_drag.as_ref().is_some_and(|file_drag| {
            file_drag.is_dragging()
                && file_drag.sources == paths
                && file_drag.native_dnd.session_id() == Some(session_id)
                && file_drag
                    .wayland_target
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.session_id == session_id)
        })
    }
}

fn wayland_drop_position(position: Option<WaylandDndDropPosition>) -> Option<Point> {
    position.map(|position| Point::new(position.x as f32, position.y as f32))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use desktop_linux::{
        FileClipboardOperation, FileClipboardSelection, WaylandDndDropOrigin,
        WaylandDndDropPosition, WaylandDndFileDrop, WaylandDndWindowHandle,
        WaylandFileDragSessionId,
    };
    use file_core::{DirectoryEntry, EntryMetadata, FileKind};
    use iced::futures::StreamExt;
    use iced::{Point, Rectangle, Size, Task};
    use iced_runtime::Action;

    use super::*;
    use crate::app::wayland_dnd::WaylandFileDragRequest;
    use crate::config;
    use crate::model::{
        FileDragNativeDndState, FileDragPhase, FileDragState, FileDragTarget, FileDropPrompt,
        SidebarFileDragTargetBounds, TransferConflictItem, TransferConflictMode,
        WaylandFileDragHitTestBounds, WaylandFileDragTargetSnapshot,
    };
    use crate::operation_queue::QueuedTransfer;

    fn test_entry(path: &Path) -> DirectoryEntry {
        DirectoryEntry::new(
            path.to_path_buf(),
            FileKind::File,
            EntryMetadata {
                len: 0,
                modified: None,
                ..EntryMetadata::default()
            },
            false,
            false,
            false,
        )
    }

    fn browser_with_entries(paths: &[PathBuf]) -> FileBrowser {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        browser.current_dir = PathBuf::from("/workspace");
        browser.entries = paths.iter().map(|path| test_entry(path)).collect();
        browser.selected_paths = paths.iter().cloned().collect::<HashSet<_>>();
        browser.selected = paths.first().cloned();
        browser
    }

    fn requested_file_drag_session(
        browser: &mut FileBrowser,
        paths: Vec<PathBuf>,
    ) -> WaylandFileDragSessionId {
        drop(browser.accept_wayland_dnd_handle(Ok(Some(WaylandDndWindowHandle::new(1, 2)))));
        match browser.request_wayland_file_drag(paths) {
            WaylandFileDragRequest::Requested(session_id) => session_id,
            WaylandFileDragRequest::Unavailable | WaylandFileDragRequest::Rejected(_) => {
                panic!("expected Wayland file drag request")
            }
        }
    }

    fn wayland_file_drop(
        operation: FileClipboardOperation,
        paths: Vec<PathBuf>,
        origin: WaylandDndDropOrigin,
    ) -> WaylandDndFileDrop {
        WaylandDndFileDrop {
            selection: FileClipboardSelection::new(operation, paths),
            origin,
            position: None,
        }
    }

    fn accept_measured_wayland_file_drop(
        browser: &mut FileBrowser,
        file_drop: WaylandDndFileDrop,
        bounds: Vec<BreadcrumbDropTargetBounds>,
    ) -> Task<Message> {
        browser.breadcrumb_drop_target_bounds = bounds.clone();
        let drop_task = browser.accept_wayland_file_drop(Ok(file_drop));
        if browser.pending_wayland_file_drop.is_none() {
            return drop_task;
        }
        drop(drop_task);
        let generation = browser.breadcrumb_drop_target_measurement_generation;
        browser.accept_breadcrumb_drop_target_bounds(generation, bounds)
    }

    fn breadcrumb_target_bounds(
        pane_id: crate::model::BrowserPaneId,
        directory: impl Into<PathBuf>,
    ) -> BreadcrumbDropTargetBounds {
        BreadcrumbDropTargetBounds {
            pane_id,
            directory: directory.into(),
            item_bounds: Rectangle::new(Point::new(300.0, 8.0), Size::new(100.0, 24.0)),
            viewport_bounds: Rectangle::new(Point::new(280.0, 0.0), Size::new(160.0, 40.0)),
        }
    }

    fn file_drag_hit_test_bounds_for_breadcrumb(
        pane_id: crate::model::BrowserPaneId,
        pane_directory: impl Into<PathBuf>,
        target_directory: impl Into<PathBuf>,
    ) -> WaylandFileDragHitTestBounds {
        WaylandFileDragHitTestBounds {
            breadcrumbs: vec![breadcrumb_target_bounds(pane_id, target_directory)],
            directory_targets: vec![crate::model::DirectoryFileDragTargetBounds {
                pane_id,
                directory: pane_directory.into(),
                bounds: Rectangle::new(Point::new(280.0, 0.0), Size::new(500.0, 500.0)),
            }],
            ..WaylandFileDragHitTestBounds::default()
        }
    }

    async fn transfer_conflict_check_message(
        task: Task<Message>,
    ) -> (
        TransferConflictMode,
        Vec<QueuedTransfer>,
        Vec<TransferConflictItem>,
    ) {
        let Some(mut stream) = iced_runtime::task::into_stream(task) else {
            panic!("expected a transfer conflict check task");
        };

        while let Some(action) = stream.next().await {
            if let Action::Output(Message::TransferConflictsChecked {
                mode,
                transfers,
                conflicts,
            }) = action
            {
                return (mode, transfers, conflicts);
            }
        }

        panic!("expected TransferConflictsChecked output");
    }

    #[test]
    fn wayland_file_drop_opens_operation_prompt() {
        let source = PathBuf::from("/outside/report.txt");
        let mut browser = browser_with_entries(&[]);
        browser.cursor_paste_directory = Some(PathBuf::from("/workspace/project"));

        drop(accept_measured_wayland_file_drop(
            &mut browser,
            wayland_file_drop(
                FileClipboardOperation::Copy,
                vec![source.clone()],
                WaylandDndDropOrigin::External,
            ),
            Vec::new(),
        ));

        assert!(matches!(
            &browser.file_drop_prompt,
            Some(FileDropPrompt {
                paste_directory,
                paths,
            }) if paste_directory == &PathBuf::from("/workspace/project")
                && paths == &vec![source]
        ));
    }

    #[test]
    fn external_wayland_drop_uses_measured_breadcrumb_target() {
        let source = PathBuf::from("/outside/report.txt");
        let target_directory = PathBuf::from("/workspace/project");
        let mut browser = browser_with_entries(&[]);
        let pane_id = browser.active_pane_id();
        let mut file_drop = wayland_file_drop(
            FileClipboardOperation::Copy,
            vec![source.clone()],
            WaylandDndDropOrigin::External,
        );
        file_drop.position = Some(WaylandDndDropPosition { x: 320.0, y: 20.0 });

        drop(accept_measured_wayland_file_drop(
            &mut browser,
            file_drop,
            vec![breadcrumb_target_bounds(pane_id, target_directory.clone())],
        ));

        assert!(matches!(
            &browser.file_drop_prompt,
            Some(FileDropPrompt {
                paste_directory,
                paths,
            }) if paste_directory == &target_directory && paths == &vec![source]
        ));
    }

    #[test]
    fn stale_breadcrumb_measurement_does_not_consume_pending_wayland_drop() {
        let source = PathBuf::from("/outside/report.txt");
        let mut browser = browser_with_entries(&[]);

        drop(browser.accept_wayland_file_drop(Ok(wayland_file_drop(
            FileClipboardOperation::Copy,
            vec![source.clone()],
            WaylandDndDropOrigin::External,
        ))));
        let current_generation = browser.breadcrumb_drop_target_measurement_generation;

        drop(
            browser.accept_breadcrumb_drop_target_bounds(
                current_generation.wrapping_sub(1),
                Vec::new(),
            ),
        );

        assert!(browser.pending_wayland_file_drop.is_some());
        assert!(browser.file_drop_prompt.is_none());

        drop(browser.accept_breadcrumb_drop_target_bounds(current_generation, Vec::new()));

        assert!(browser.pending_wayland_file_drop.is_none());
        assert!(matches!(
            &browser.file_drop_prompt,
            Some(FileDropPrompt { paths, .. }) if paths == &vec![source]
        ));
    }

    #[test]
    fn second_wayland_file_drop_keeps_pending_prompt() {
        let first_source = PathBuf::from("/outside/first.txt");
        let second_source = PathBuf::from("/outside/second.txt");
        let mut browser = browser_with_entries(&[]);

        drop(accept_measured_wayland_file_drop(
            &mut browser,
            wayland_file_drop(
                FileClipboardOperation::Move,
                vec![first_source.clone()],
                WaylandDndDropOrigin::External,
            ),
            Vec::new(),
        ));
        drop(accept_measured_wayland_file_drop(
            &mut browser,
            wayland_file_drop(
                FileClipboardOperation::Move,
                vec![second_source],
                WaylandDndDropOrigin::External,
            ),
            Vec::new(),
        ));

        assert!(matches!(
            &browser.file_drop_prompt,
            Some(FileDropPrompt { paths, .. }) if paths == &vec![first_source]
        ));
        assert_eq!(
            browser.error.as_deref(),
            Some("Finish the current file operation prompt before dropping files")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn selected_file_drop_operation_applies_immediately() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let destination = temp_dir.path().join("destination");
        let source = temp_dir.path().join("report.txt");
        fs::create_dir_all(&destination).expect("create destination");
        fs::write(&source, b"report").expect("write source");
        let mut browser = browser_with_entries(&[]);
        browser.current_dir = destination.clone();

        drop(accept_measured_wayland_file_drop(
            &mut browser,
            wayland_file_drop(
                FileClipboardOperation::Move,
                vec![source.clone()],
                WaylandDndDropOrigin::External,
            ),
            Vec::new(),
        ));

        let (mode, transfers, conflicts) = transfer_conflict_check_message(
            browser.apply_file_drop_operation(FileClipboardOperation::Copy),
        )
        .await;

        assert!(browser.file_drop_prompt.is_none());
        assert_eq!(mode, TransferConflictMode::Copy);
        assert_eq!(
            transfers,
            vec![QueuedTransfer::new(source, destination.join("report.txt"))]
        );
        assert!(conflicts.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn internal_wayland_file_drop_moves_from_lower_to_upper_column() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let upper_directory = temp_dir.path().join("upper");
        let lower_directory = upper_directory.join("lower");
        fs::create_dir_all(&lower_directory).expect("create nested directories");
        let source = lower_directory.join("report.txt");
        fs::write(&source, b"report").expect("write source");
        let mut browser = browser_with_entries(&[source.clone()]);
        let pane_id = browser.active_pane_id();
        let session_id = requested_file_drag_session(&mut browser, vec![source.clone()]);
        let drop_position = WaylandDndDropPosition { x: 300.0, y: 250.0 };
        browser.file_drag = Some(FileDragState {
            sources: vec![source.clone()],
            pressed_path: source.clone(),
            target: None,
            phase: FileDragPhase::Dragging,
            native_dnd: FileDragNativeDndState::Started(session_id),
            wayland_target: Some(WaylandFileDragTargetSnapshot {
                session_id,
                hit_test_bounds: WaylandFileDragHitTestBounds {
                    directory_targets: vec![
                        crate::model::DirectoryFileDragTargetBounds {
                            pane_id,
                            directory: temp_dir.path().to_path_buf(),
                            bounds: Rectangle::new(Point::new(0.0, 0.0), Size::new(200.0, 500.0)),
                        },
                        crate::model::DirectoryFileDragTargetBounds {
                            pane_id,
                            directory: upper_directory.clone(),
                            bounds: Rectangle::new(Point::new(205.0, 0.0), Size::new(200.0, 500.0)),
                        },
                    ],
                    ..WaylandFileDragHitTestBounds::default()
                },
                bookmark_source: None,
                position: Some(Point::new(300.0, 250.0)),
                target: Some(FileDragTarget::Directory(upper_directory.clone())),
            }),
            column_directories_snapshot: Vec::new(),
        });

        assert_eq!(
            browser.wayland_file_drag_target_at_drop(session_id, Point::new(300.0, 250.0)),
            Some(FileDragTarget::Directory(upper_directory.clone()))
        );
        let mut file_drop = wayland_file_drop(
            FileClipboardOperation::Move,
            vec![source.clone()],
            WaylandDndDropOrigin::Internal(session_id),
        );
        file_drop.position = Some(drop_position);
        let file_drop_task = accept_measured_wayland_file_drop(&mut browser, file_drop, Vec::new());
        assert!(browser.pending_wayland_file_drop.is_none());
        assert!(browser.file_drag.is_none());
        let (mode, transfers, conflicts) = transfer_conflict_check_message(file_drop_task).await;

        assert!(browser.file_drop_prompt.is_none());
        assert!(browser.file_drag.is_none());
        assert_eq!(mode, TransferConflictMode::Move);
        assert_eq!(
            transfers,
            vec![QueuedTransfer::new(
                source,
                upper_directory.join("report.txt"),
            )]
        );
        assert!(conflicts.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn internal_wayland_drop_uses_session_target_before_source_terminal() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let destination = temp_dir.path().join("destination");
        let stale_destination = temp_dir.path().join("stale-destination");
        let source = temp_dir.path().join("report.txt");
        fs::create_dir_all(&destination).expect("create destination");
        fs::create_dir_all(&stale_destination).expect("create stale destination");
        fs::write(&source, b"report").expect("write source");
        let mut browser = browser_with_entries(&[source.clone()]);
        let pane_id = browser.active_pane_id();
        let session_id = requested_file_drag_session(&mut browser, vec![source.clone()]);
        browser.file_drag = Some(FileDragState {
            sources: vec![source.clone()],
            pressed_path: source.clone(),
            target: Some(FileDragTarget::Directory(stale_destination)),
            phase: FileDragPhase::Dragging,
            native_dnd: FileDragNativeDndState::Started(session_id),
            wayland_target: Some(WaylandFileDragTargetSnapshot {
                session_id,
                hit_test_bounds: file_drag_hit_test_bounds_for_breadcrumb(
                    pane_id,
                    temp_dir.path(),
                    destination.clone(),
                ),
                bookmark_source: None,
                position: Some(Point::new(320.0, 20.0)),
                target: Some(FileDragTarget::Directory(destination.clone())),
            }),
            column_directories_snapshot: Vec::new(),
        });
        assert_eq!(
            browser.wayland_file_drag_target_at_drop(session_id, Point::new(320.0, 20.0)),
            Some(FileDragTarget::Directory(destination.clone()))
        );
        let mut file_drop = wayland_file_drop(
            FileClipboardOperation::Move,
            vec![source.clone()],
            WaylandDndDropOrigin::Internal(session_id),
        );
        file_drop.position = Some(WaylandDndDropPosition { x: 320.0, y: 20.0 });

        let file_drop_task = accept_measured_wayland_file_drop(
            &mut browser,
            file_drop,
            vec![breadcrumb_target_bounds(pane_id, destination.clone())],
        );
        let (mode, transfers, conflicts) = transfer_conflict_check_message(file_drop_task).await;

        assert!(browser.file_drag.is_none());
        assert_eq!(mode, TransferConflictMode::Move);
        assert_eq!(
            transfers,
            vec![QueuedTransfer::new(source, destination.join("report.txt"))]
        );
        assert!(conflicts.is_empty());
    }

    #[test]
    fn internal_bookmark_drop_uses_session_source_after_entries_refresh() {
        let source = PathBuf::from("/workspace/projects");
        let mut browser = browser_with_entries(&[]);
        let session_id = requested_file_drag_session(&mut browser, vec![source.clone()]);
        browser.file_drag = Some(FileDragState {
            sources: vec![source.clone()],
            pressed_path: source.clone(),
            target: None,
            phase: FileDragPhase::Dragging,
            native_dnd: FileDragNativeDndState::Started(session_id),
            wayland_target: Some(WaylandFileDragTargetSnapshot {
                session_id,
                hit_test_bounds: WaylandFileDragHitTestBounds {
                    sidebar_directories: vec![SidebarFileDragTargetBounds {
                        directory: PathBuf::from("/workspace/favorite"),
                        favorite_index: Some(0),
                        bounds: Rectangle::new(
                            Point::new(0.0, 40.0),
                            Size::new(browser.sidebar_width, 32.0),
                        ),
                    }],
                    ..WaylandFileDragHitTestBounds::default()
                },
                bookmark_source: Some(source.clone()),
                position: None,
                target: None,
            }),
            column_directories_snapshot: Vec::new(),
        });
        let mut drop_payload = wayland_file_drop(
            FileClipboardOperation::Move,
            vec![source.clone()],
            WaylandDndDropOrigin::Internal(session_id),
        );
        drop_payload.position = Some(WaylandDndDropPosition { x: 20.0, y: 42.0 });

        drop(browser.accept_wayland_file_drop(Ok(drop_payload)));

        assert!(browser.file_drag.is_none());
        assert!(browser
            .sidebar_locations
            .iter()
            .any(|location| location.path == source));
        assert!(browser.operation_queue.tasks().is_empty());
    }

    #[test]
    fn stale_internal_drop_with_matching_paths_does_not_consume_active_session() {
        let source = PathBuf::from("/workspace/report.txt");
        let mut browser = browser_with_entries(std::slice::from_ref(&source));
        let stale_session_id = requested_file_drag_session(&mut browser, vec![source.clone()]);
        let active_session_id = requested_file_drag_session(&mut browser, vec![source.clone()]);
        browser.file_drag = Some(FileDragState {
            sources: vec![source.clone()],
            pressed_path: source.clone(),
            target: None,
            phase: FileDragPhase::Dragging,
            native_dnd: FileDragNativeDndState::Started(active_session_id),
            wayland_target: Some(WaylandFileDragTargetSnapshot {
                session_id: active_session_id,
                hit_test_bounds: WaylandFileDragHitTestBounds::default(),
                bookmark_source: None,
                position: None,
                target: None,
            }),
            column_directories_snapshot: Vec::new(),
        });

        drop(browser.accept_wayland_file_drop(Ok(wayland_file_drop(
            FileClipboardOperation::Move,
            vec![source],
            WaylandDndDropOrigin::Internal(stale_session_id),
        ))));

        assert_eq!(
            browser.file_drag.as_ref().map(|drag| drag.native_dnd),
            Some(FileDragNativeDndState::Started(active_session_id))
        );
        assert!(browser.operation_queue.tasks().is_empty());
    }

    #[test]
    fn internal_drop_on_later_column_blank_preserves_source_without_operation() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let child_directory = temp_dir.path().join("child");
        fs::create_dir(&child_directory).expect("create child directory");
        let source = child_directory.join("unique-stationary-source.txt");
        fs::write(&source, b"stationary source").expect("write source");
        let mut browser = browser_with_entries(std::slice::from_ref(&source));
        let pane_id = browser.active_pane_id();
        let session_id = requested_file_drag_session(&mut browser, vec![source.clone()]);
        browser.file_drag = Some(FileDragState {
            sources: vec![source.clone()],
            pressed_path: source.clone(),
            target: None,
            phase: FileDragPhase::Dragging,
            native_dnd: FileDragNativeDndState::Started(session_id),
            wayland_target: Some(WaylandFileDragTargetSnapshot {
                session_id,
                hit_test_bounds: WaylandFileDragHitTestBounds {
                    directory_targets: vec![
                        crate::model::DirectoryFileDragTargetBounds {
                            pane_id,
                            directory: temp_dir.path().to_path_buf(),
                            bounds: Rectangle::new(Point::new(0.0, 0.0), Size::new(200.0, 500.0)),
                        },
                        crate::model::DirectoryFileDragTargetBounds {
                            pane_id,
                            directory: child_directory,
                            bounds: Rectangle::new(Point::new(205.0, 0.0), Size::new(200.0, 500.0)),
                        },
                    ],
                    ..WaylandFileDragHitTestBounds::default()
                },
                bookmark_source: None,
                position: None,
                target: None,
            }),
            column_directories_snapshot: Vec::new(),
        });
        let mut drop_payload = wayland_file_drop(
            FileClipboardOperation::Move,
            vec![source.clone()],
            WaylandDndDropOrigin::Internal(session_id),
        );
        drop_payload.position = Some(WaylandDndDropPosition { x: 300.0, y: 250.0 });

        drop(browser.accept_wayland_file_drop(Ok(drop_payload)));

        assert!(browser.file_drag.is_none());
        assert!(browser.operation_queue.tasks().is_empty());
        assert_eq!(
            fs::read(&source).expect("read preserved source"),
            b"stationary source"
        );
        assert!(!temp_dir
            .path()
            .join("unique-stationary-source.txt")
            .exists());
    }

    #[test]
    fn internal_drop_without_snapshot_target_consumes_session_without_operation() {
        let source = PathBuf::from("/workspace/report.txt");
        let mut browser = browser_with_entries(std::slice::from_ref(&source));
        let session_id = requested_file_drag_session(&mut browser, vec![source.clone()]);
        browser.file_drag = Some(FileDragState {
            sources: vec![source.clone()],
            pressed_path: source.clone(),
            target: None,
            phase: FileDragPhase::Dragging,
            native_dnd: FileDragNativeDndState::Started(session_id),
            wayland_target: Some(WaylandFileDragTargetSnapshot {
                session_id,
                hit_test_bounds: WaylandFileDragHitTestBounds::default(),
                bookmark_source: None,
                position: None,
                target: None,
            }),
            column_directories_snapshot: Vec::new(),
        });
        let mut drop_payload = wayland_file_drop(
            FileClipboardOperation::Move,
            vec![source],
            WaylandDndDropOrigin::Internal(session_id),
        );
        drop_payload.position = Some(WaylandDndDropPosition { x: -10.0, y: -10.0 });

        drop(browser.accept_wayland_file_drop(Ok(drop_payload)));

        assert!(browser.file_drag.is_none());
        assert!(browser.operation_queue.tasks().is_empty());
    }

    #[test]
    fn cancelled_file_drop_clears_prompt() {
        let source = PathBuf::from("/outside/report.txt");
        let mut browser = browser_with_entries(&[]);

        drop(accept_measured_wayland_file_drop(
            &mut browser,
            wayland_file_drop(
                FileClipboardOperation::Move,
                vec![source],
                WaylandDndDropOrigin::External,
            ),
            Vec::new(),
        ));
        drop(browser.cancel_file_drop());

        assert!(browser.file_drop_prompt.is_none());
    }
}
