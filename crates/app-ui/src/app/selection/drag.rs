use std::path::{Path, PathBuf};

use iced::Task;

use super::super::paths::{self, PasteTargetMode};
use super::super::wayland_dnd::WaylandFileDragRequest;
use super::super::{FileBrowser, POINTER_DRAG_ACTIVATION_DISTANCE};
use crate::file_drag_hit_test_bounds::{
    file_drag_hit_test_bounds_command, FileDragHitTestBoundsRequest,
};
use crate::model::{
    BrowserPaneId, FileDragHitTestBounds, FileDragNativeDndState, FileDragPhase, FileDragState,
    FileDragStationaryAction, FileDragTarget, Message, SelectionMarqueeSource,
    TransferConflictMode, WaylandFileDragEntryTargetBounds, WaylandFileDragHitTestBounds,
    WaylandFileDragTargetSnapshot,
};
use crate::operation_queue::QueuedTransfer;

struct FileDragDirectoryPositionTarget {
    directory: PathBuf,
}

impl FileBrowser {
    pub(crate) fn update_file_drag(&mut self, position: iced::Point) -> Task<Message> {
        {
            let Some(file_drag) = &mut self.file_drag else {
                return Task::none();
            };
            let FileDragPhase::WaitingForMovement { origin } = file_drag.phase else {
                return Task::none();
            };

            let delta_x = position.x - origin.x;
            let delta_y = position.y - origin.y;
            if delta_x * delta_x + delta_y * delta_y
                < POINTER_DRAG_ACTIVATION_DISTANCE * POINTER_DRAG_ACTIVATION_DISTANCE
            {
                return Task::none();
            }

            file_drag.phase = FileDragPhase::Dragging;
        }

        self.prepare_native_file_drag_after_activation()
    }

    fn prepare_native_file_drag_after_activation(&mut self) -> Task<Message> {
        let can_prepare_native_drag = self.wayland_dnd.is_some()
            && self
                .file_drag
                .as_ref()
                .is_some_and(FileDragState::can_start_native_dnd);
        if !can_prepare_native_drag {
            return crate::column_entry_bounds::column_entry_bounds_command();
        }

        self.native_file_drag_target_measurement_generation = self
            .native_file_drag_target_measurement_generation
            .wrapping_add(1);
        let measurement_id = self.native_file_drag_target_measurement_generation;
        if let Some(file_drag) = &mut self.file_drag {
            file_drag.native_dnd = FileDragNativeDndState::MeasuringTargets(measurement_id);
        }
        file_drag_hit_test_bounds_command(FileDragHitTestBoundsRequest::NativeFileDrag(
            measurement_id,
        ))
    }

    pub(in crate::app) fn accept_native_bounds(
        &mut self,
        measurement_id: u64,
        hit_test_bounds: FileDragHitTestBounds,
    ) -> Task<Message> {
        let drag_sources = self.file_drag.as_ref().and_then(|file_drag| {
            (file_drag.native_dnd == FileDragNativeDndState::MeasuringTargets(measurement_id))
                .then(|| file_drag.sources.clone())
        });
        let Some(drag_sources) = drag_sources else {
            return Task::none();
        };

        let bookmark_source = (drag_sources.len() == 1
            && self.entry_kind(&drag_sources[0]) == Some(file_core::FileKind::Directory))
        .then(|| drag_sources[0].clone());
        let entries = hit_test_bounds
            .entries
            .iter()
            .filter_map(|entry| {
                self.file_drag_release_directory_for_entry(entry.pane_id, &entry.path)
                    .map(|directory| WaylandFileDragEntryTargetBounds {
                        directory,
                        path: entry.path.clone(),
                        bounds: entry.bounds,
                    })
            })
            .collect();
        let directory_targets = hit_test_bounds
            .directory_targets
            .into_iter()
            .filter(|target| self.pane_accepts_file_drag(target.pane_id))
            .collect();
        let blocked_directories = hit_test_bounds
            .blocked_directories
            .into_iter()
            .filter(|blocked| self.pane_accepts_file_drag(blocked.pane_id))
            .collect();
        let wayland_hit_test_bounds = WaylandFileDragHitTestBounds {
            entries,
            breadcrumbs: hit_test_bounds.breadcrumbs,
            directory_targets,
            blocked_directories,
            sidebar_directories: hit_test_bounds.sidebar_directories,
            empty_sidebar_bookmarks: hit_test_bounds.empty_sidebar_bookmarks,
        };
        match self.request_wayland_file_drag(drag_sources) {
            WaylandFileDragRequest::Unavailable => {
                if let Some(file_drag) = &mut self.file_drag {
                    file_drag.native_dnd = FileDragNativeDndState::NotRequested;
                }
            }
            WaylandFileDragRequest::Requested(session_id) => {
                if let Some(file_drag) = &mut self.file_drag {
                    file_drag.native_dnd = FileDragNativeDndState::Requested(session_id);
                    file_drag.wayland_target = Some(WaylandFileDragTargetSnapshot {
                        session_id,
                        hit_test_bounds: wayland_hit_test_bounds,
                        bookmark_source,
                        position: None,
                        target: None,
                    });
                }
            }
            WaylandFileDragRequest::Rejected(error) => {
                self.file_drag = None;
                self.drag_selection_anchor = None;
                self.sidebar_bookmark_drop_slot = None;
                self.show_global_error(error);
            }
        }

        Task::none()
    }

    pub(crate) fn finish_drag_selection(
        &mut self,
        release_directory: Option<PathBuf>,
    ) -> Task<Message> {
        let native_dnd = self
            .file_drag
            .as_ref()
            .map(|file_drag| file_drag.native_dnd);
        if native_dnd.is_some_and(|state| state.session_id().is_some()) {
            return Task::none();
        }
        if matches!(
            native_dnd,
            Some(FileDragNativeDndState::MeasuringTargets(_))
        ) {
            self.drag_selection_anchor = None;
            self.selection_marquee = None;
            self.sidebar_bookmark_drop_slot = None;
            self.file_drag = None;
            return Task::none();
        }

        let column_blank_click = self.selection_marquee.as_ref().and_then(|marquee| {
            if marquee.is_selecting() {
                return None;
            }
            match &marquee.source {
                SelectionMarqueeSource::PaneBlank
                | SelectionMarqueeSource::IconGridPanel { .. } => None,
                SelectionMarqueeSource::ColumnBlank { directory } => Some(directory.clone()),
            }
        });
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.sidebar_bookmark_drop_slot = None;
        if let Some(directory) = column_blank_click {
            return self.handle_column_blank_clicked(directory);
        }
        let Some(file_drag) = self.file_drag.take() else {
            return Task::none();
        };

        if !file_drag.is_dragging() {
            return self.finish_stationary_file_drag(file_drag);
        }

        self.finish_active_file_drag(file_drag, release_directory)
    }

    pub(in crate::app) fn finish_wayland_file_drag(
        &mut self,
        session_id: desktop_linux::WaylandFileDragSessionId,
        target: Option<FileDragTarget>,
    ) -> Task<Message> {
        let session_matches = self.file_drag.as_ref().is_some_and(|file_drag| {
            file_drag.native_dnd.session_id() == Some(session_id)
                && file_drag
                    .wayland_target
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.session_id == session_id)
        });
        if !session_matches {
            return Task::none();
        }

        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.sidebar_bookmark_drop_slot = None;
        let file_drag = self.file_drag.take().expect("matching Wayland file drag");
        let Some(target) = safe_file_drag_target(&file_drag.sources, target) else {
            return Task::none();
        };
        match target {
            FileDragTarget::Directory(target_directory) => {
                self.move_dragged_files(file_drag.sources, target_directory)
            }
            FileDragTarget::SidebarBookmarkSlot(slot) => file_drag
                .wayland_target
                .and_then(|snapshot| snapshot.bookmark_source)
                .map_or_else(Task::none, |source| {
                    self.insert_sidebar_bookmark_from_drag(slot, source)
                }),
        }
    }

    fn apply_file_drag_target(
        &mut self,
        sources: Vec<PathBuf>,
        target: FileDragTarget,
    ) -> Task<Message> {
        match target {
            FileDragTarget::Directory(target_directory) => {
                self.move_dragged_files(sources, target_directory)
            }
            FileDragTarget::SidebarBookmarkSlot(slot) => {
                self.add_dragged_sidebar_bookmark(slot, sources)
            }
        }
    }

    fn finish_active_file_drag(
        &mut self,
        file_drag: FileDragState,
        release_directory: Option<PathBuf>,
    ) -> Task<Message> {
        let cursor_fallback_directory = release_directory
            .clone()
            .or_else(|| self.file_drag_drop_directory_at_cursor());
        let Some(target) = resolve_file_drag_target(
            &file_drag.sources,
            release_directory,
            file_drag.target,
            cursor_fallback_directory,
        ) else {
            return Task::none();
        };

        self.apply_file_drag_target(file_drag.sources, target)
    }

    pub(crate) fn start_file_drag(
        &mut self,
        pressed_path: PathBuf,
        stationary_action: FileDragStationaryAction,
        column_directories_snapshot: Vec<PathBuf>,
    ) {
        self.sidebar_bookmark_drop_slot = None;
        if self.is_trash_view {
            self.file_drag = None;
            return;
        }

        let sources = self.selected_paths_for_operation();
        self.file_drag = (!sources.is_empty()).then_some(FileDragState {
            sources,
            pressed_path,
            stationary_action,
            target: None,
            phase: FileDragPhase::WaitingForMovement {
                origin: self.cursor_position,
            },
            native_dnd: FileDragNativeDndState::NotRequested,
            wayland_target: None,
            column_directories_snapshot,
        });
    }

    fn finish_stationary_file_drag(&mut self, file_drag: FileDragState) -> Task<Message> {
        if file_drag.sources.len() > 1
            && file_drag
                .sources
                .iter()
                .any(|source| source == &file_drag.pressed_path)
        {
            self.select_path(file_drag.pressed_path.clone());
        }

        match file_drag.stationary_action {
            FileDragStationaryAction::SelectionOnly => Task::none(),
            FileDragStationaryAction::ActivateColumnEntry => {
                self.update_open_column_directory_for_entry(&file_drag.pressed_path);
                Task::batch([
                    self.open_column_for_directory(file_drag.pressed_path),
                    self.request_browser_session_save(),
                ])
            }
        }
    }

    pub(crate) fn set_file_drag_target(&mut self, directory: PathBuf) {
        if let Some(file_drag) = &mut self.file_drag {
            file_drag.target = Some(FileDragTarget::Directory(directory));
        }
    }

    pub(crate) fn clear_file_drag_target(&mut self) {
        if let Some(file_drag) = &mut self.file_drag {
            file_drag.target = None;
        }
    }

    pub(crate) fn clear_file_drag_target_if_matching(&mut self, directory: &Path) {
        if let Some(file_drag) = &mut self.file_drag {
            if matches!(file_drag.target.as_ref(), Some(FileDragTarget::Directory(target)) if target == directory)
            {
                file_drag.target = None;
            }
        }
    }

    pub(crate) fn file_drag_release_directory_for_entry(
        &self,
        pane_id: BrowserPaneId,
        path: &Path,
    ) -> Option<PathBuf> {
        self.cursor_paste_directory_for_entry_in_pane(pane_id, path)
    }

    pub(crate) fn file_drag_release_directory_for_drop_target(
        &self,
        pane_id: BrowserPaneId,
        directory: PathBuf,
    ) -> Option<PathBuf> {
        self.pane_accepts_file_drag(pane_id).then_some(directory)
    }

    pub(crate) fn directory_drop_target_at_position(
        &self,
        position: iced::Point,
    ) -> Option<PathBuf> {
        self.file_drag_directory_target_at_position(
            position,
            &self.file_entry_bounds,
            &self.breadcrumb_drop_target_bounds,
        )
        .map(|target| target.directory)
    }

    fn file_drag_directory_target_at_position(
        &self,
        position: iced::Point,
        entry_bounds: &[crate::model::ColumnEntryBounds],
        breadcrumb_bounds: &[crate::model::BreadcrumbDropTargetBounds],
    ) -> Option<FileDragDirectoryPositionTarget> {
        if let Some(directory) =
            self.breadcrumb_drop_directory_in_bounds(position, breadcrumb_bounds)
        {
            return Some(FileDragDirectoryPositionTarget { directory });
        }

        for entry_bounds in entry_bounds.iter().rev() {
            if !entry_bounds.bounds.contains(position) {
                continue;
            }
            return self
                .file_drag_release_directory_for_entry(entry_bounds.pane_id, &entry_bounds.path)
                .map(|directory| FileDragDirectoryPositionTarget { directory });
        }

        let pane_id = self.pane_id_at_position(position)?;
        self.pane_view(pane_id)
            .filter(|pane| !pane.is_trash_view)
            .map(|pane| FileDragDirectoryPositionTarget {
                directory: pane.current_dir.clone(),
            })
    }

    #[cfg(test)]
    fn breadcrumb_drop_directory_at_position(&self, position: iced::Point) -> Option<PathBuf> {
        self.breadcrumb_drop_directory_in_bounds(position, &self.breadcrumb_drop_target_bounds)
    }

    fn breadcrumb_drop_directory_in_bounds(
        &self,
        position: iced::Point,
        breadcrumb_bounds: &[crate::model::BreadcrumbDropTargetBounds],
    ) -> Option<PathBuf> {
        breadcrumb_bounds
            .iter()
            .filter(|target| {
                target.item_bounds.contains(position)
                    && target.viewport_bounds.contains(position)
                    && self
                        .pane_view(target.pane_id)
                        .is_some_and(|pane| !pane.is_trash_view)
            })
            .max_by_key(|target| target.directory.components().count())
            .map(|target| target.directory.clone())
    }

    fn file_drag_drop_directory_at_cursor(&self) -> Option<PathBuf> {
        let pane_id = self.pane_id_at_position(self.cursor_position)?;
        if pane_id == self.active_pane_id() {
            return None;
        }

        let pane = self.pane_view(pane_id)?;
        (!pane.is_trash_view).then(|| pane.current_dir.clone())
    }

    fn move_dragged_files(
        &mut self,
        sources: Vec<PathBuf>,
        target_directory: PathBuf,
    ) -> Task<Message> {
        let transfer_targets =
            paths::transfer_targets(&target_directory, &sources, PasteTargetMode::Move);
        if transfer_targets.is_empty()
            || transfer_targets.iter().any(|(source, target)| {
                source == target
                    || target.starts_with(source)
                    || source
                        .parent()
                        .is_some_and(|parent| parent == target_directory)
            })
        {
            return Task::none();
        }
        let transfers = transfer_targets
            .into_iter()
            .map(|(source, target)| QueuedTransfer::new(source, target))
            .collect::<Vec<_>>();

        let open_drop_target = if sources
            .first()
            .and_then(|source| source.parent())
            .is_some_and(|source_parent| {
                target_directory != source_parent && target_directory.starts_with(source_parent)
            }) {
            self.select_path(target_directory.clone());
            self.open_column_for_directory(target_directory)
        } else {
            Task::none()
        };
        Task::batch([
            open_drop_target,
            self.enqueue_or_confirm_transfers(TransferConflictMode::Move, transfers),
        ])
    }

    pub(crate) fn extend_drag_selection_to(&mut self, path: PathBuf) {
        let Some(anchor) = self.drag_selection_anchor.clone() else {
            if self.selection_marquee.is_some() {
                self.drag_selection_anchor = Some(path.clone());
                self.select_drag_range(path.clone(), path, self.keyboard_modifiers.control());
            }
            return;
        };
        self.select_drag_range(anchor, path, self.keyboard_modifiers.control());
    }
}
pub(super) fn resolve_file_drag_target(
    sources: &[PathBuf],
    release_directory: Option<PathBuf>,
    target: Option<FileDragTarget>,
    fallback_directory: Option<PathBuf>,
) -> Option<FileDragTarget> {
    if let Some(release_directory) = release_directory {
        return Some(FileDragTarget::Directory(release_directory));
    }

    match target {
        Some(FileDragTarget::Directory(target_directory)) => {
            if file_drag_directory_target_needs_fallback(sources, &target_directory) {
                if let Some(fallback_directory) = fallback_directory {
                    Some(FileDragTarget::Directory(fallback_directory))
                } else {
                    Some(FileDragTarget::Directory(target_directory))
                }
            } else {
                Some(FileDragTarget::Directory(target_directory))
            }
        }
        Some(FileDragTarget::SidebarBookmarkSlot(slot)) => {
            Some(FileDragTarget::SidebarBookmarkSlot(slot))
        }
        None => fallback_directory.map(FileDragTarget::Directory),
    }
}

pub(super) fn safe_file_drag_target(
    sources: &[PathBuf],
    target: Option<FileDragTarget>,
) -> Option<FileDragTarget> {
    match target {
        Some(FileDragTarget::Directory(directory))
            if file_drag_directory_target_needs_fallback(sources, &directory) =>
        {
            None
        }
        target => target,
    }
}

fn file_drag_directory_target_needs_fallback(sources: &[PathBuf], target: &Path) -> bool {
    sources.iter().any(|source| {
        source == target
            || target.starts_with(source)
            || source.parent().is_some_and(|parent| parent == target)
    })
}

#[cfg(test)]
mod file_drag_safety_tests {
    use super::*;

    #[test]
    fn unsafe_directory_targets_are_rejected_for_every_source_relationship() {
        let file_source = PathBuf::from("/workspace/report.txt");
        let directory_source = PathBuf::from("/workspace/project");

        for (sources, target) in [
            (vec![file_source.clone()], PathBuf::from("/workspace")),
            (vec![directory_source.clone()], directory_source.clone()),
            (
                vec![directory_source.clone()],
                directory_source.join("nested"),
            ),
        ] {
            assert!(
                safe_file_drag_target(&sources, Some(FileDragTarget::Directory(target)),).is_none()
            );
        }
    }
}

#[cfg(test)]
mod breadcrumb_drop_target_tests {
    use iced::{keyboard, Point, Rectangle, Size};

    use super::*;
    use crate::config;
    use crate::model::BreadcrumbDropTargetBounds;

    fn rectangle(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(width, height))
    }

    fn browser_with_breadcrumb_targets(targets: Vec<BreadcrumbDropTargetBounds>) -> FileBrowser {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        browser.breadcrumb_drop_target_bounds = targets;
        browser
    }

    fn breadcrumb_target(
        pane_id: BrowserPaneId,
        directory: impl Into<PathBuf>,
        item_bounds: Rectangle,
        viewport_bounds: Rectangle,
    ) -> BreadcrumbDropTargetBounds {
        BreadcrumbDropTargetBounds {
            pane_id,
            directory: directory.into(),
            item_bounds,
            viewport_bounds,
        }
    }

    #[test]
    fn breadcrumb_target_requires_item_and_viewport_hit() {
        let (browser, _) = FileBrowser::new(config::default_user_config());
        let pane_id = browser.active_pane_id();
        let target = breadcrumb_target(
            pane_id,
            "/workspace/project",
            rectangle(90.0, 10.0, 40.0, 24.0),
            rectangle(0.0, 0.0, 100.0, 40.0),
        );
        let browser = browser_with_breadcrumb_targets(vec![target]);

        assert_eq!(
            browser.breadcrumb_drop_directory_at_position(Point::new(95.0, 20.0)),
            Some(PathBuf::from("/workspace/project"))
        );
        assert_eq!(
            browser.breadcrumb_drop_directory_at_position(Point::new(120.0, 20.0)),
            None
        );
    }

    #[test]
    fn deepest_overlapping_breadcrumb_target_wins() {
        let (browser, _) = FileBrowser::new(config::default_user_config());
        let pane_id = browser.active_pane_id();
        let viewport_bounds = rectangle(0.0, 0.0, 200.0, 40.0);
        let item_bounds = rectangle(20.0, 8.0, 100.0, 24.0);
        let browser = browser_with_breadcrumb_targets(vec![
            breadcrumb_target(pane_id, "/workspace", item_bounds, viewport_bounds),
            breadcrumb_target(
                pane_id,
                "/workspace/project/src",
                item_bounds,
                viewport_bounds,
            ),
        ]);

        assert_eq!(
            browser.breadcrumb_drop_directory_at_position(Point::new(50.0, 20.0)),
            Some(PathBuf::from("/workspace/project/src"))
        );
    }

    #[test]
    fn inactive_split_pane_breadcrumb_resolves_without_activation() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let original_pane_id = browser.active_pane_id();
        browser.keyboard_modifiers = keyboard::Modifiers::SHIFT;
        drop(browser.open_directory_from_middle_click(PathBuf::from("/workspace/other")));
        let active_pane_id = browser.active_pane_id();
        assert_ne!(active_pane_id, original_pane_id);

        let target_directory = PathBuf::from("/workspace/project");
        browser.breadcrumb_drop_target_bounds = vec![breadcrumb_target(
            original_pane_id,
            target_directory.clone(),
            rectangle(20.0, 8.0, 100.0, 24.0),
            rectangle(0.0, 0.0, 200.0, 40.0),
        )];

        assert_eq!(
            browser.breadcrumb_drop_directory_at_position(Point::new(50.0, 20.0)),
            Some(target_directory)
        );
        assert_eq!(browser.active_pane_id(), active_pane_id);
    }

    #[test]
    fn trash_pane_rejects_breadcrumb_target() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        browser.is_trash_view = true;
        let pane_id = browser.active_pane_id();
        browser.breadcrumb_drop_target_bounds = vec![breadcrumb_target(
            pane_id,
            "/workspace",
            rectangle(20.0, 8.0, 100.0, 24.0),
            rectangle(0.0, 0.0, 200.0, 40.0),
        )];

        assert_eq!(
            browser.breadcrumb_drop_directory_at_position(Point::new(50.0, 20.0)),
            None
        );
    }

    #[test]
    fn clearing_breadcrumb_hover_only_clears_matching_drag_target() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let source = PathBuf::from("/workspace/report.txt");
        let target = PathBuf::from("/workspace/project");
        browser.file_drag = Some(FileDragState {
            sources: vec![source.clone()],
            pressed_path: source,
            stationary_action: FileDragStationaryAction::SelectionOnly,
            target: Some(FileDragTarget::Directory(target.clone())),
            phase: FileDragPhase::Dragging,
            native_dnd: FileDragNativeDndState::NotRequested,
            wayland_target: None,
            column_directories_snapshot: Vec::new(),
        });

        drop(browser.handle_drop_target_hover_cleared(PathBuf::from("/workspace/other")));
        assert!(matches!(
            browser
                .file_drag
                .as_ref()
                .and_then(|file_drag| file_drag.target.as_ref()),
            Some(FileDragTarget::Directory(directory)) if directory == &target
        ));

        drop(browser.handle_drop_target_hover_cleared(target));
        assert!(browser
            .file_drag
            .as_ref()
            .is_some_and(|file_drag| file_drag.target.is_none()));
    }
}
