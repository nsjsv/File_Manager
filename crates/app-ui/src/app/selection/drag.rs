use std::path::{Path, PathBuf};

use iced::Task;

use super::super::paths::{self, PasteTargetMode};
use super::super::{FileBrowser, POINTER_DRAG_ACTIVATION_DISTANCE};
use crate::model::{
    BrowserPaneId, FileDragNativeDndState, FileDragPhase, FileDragState, FileDragTarget, Message,
    SelectionMarqueeSource, TransferConflictMode,
};
use crate::operation_queue::QueuedTransfer;

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

        crate::column_entry_bounds::column_entry_bounds_command()
    }

    pub(crate) fn request_file_drag_wayland_dnd_on_window_exit(&mut self) -> Task<Message> {
        let drag_sources = self
            .file_drag
            .as_ref()
            .filter(|file_drag| file_drag.can_request_wayland_dnd())
            .map(|file_drag| file_drag.sources.clone());
        let Some(drag_sources) = drag_sources else {
            return Task::none();
        };

        if self.request_wayland_file_drag(drag_sources) {
            if let Some(file_drag) = &mut self.file_drag {
                file_drag.phase = FileDragPhase::Dragging;
                file_drag.native_dnd = FileDragNativeDndState::WaylandRequested;
            }
        }

        Task::none()
    }

    pub(crate) fn finish_drag_selection(
        &mut self,
        release_directory: Option<PathBuf>,
    ) -> Task<Message> {
        let column_blank_click = self.selection_marquee.as_ref().and_then(|marquee| {
            if marquee.is_selecting() {
                return None;
            }
            match &marquee.source {
                SelectionMarqueeSource::PaneBlank => None,
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
            self.finish_stationary_file_drag(file_drag);
            return Task::none();
        }

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

        match target {
            FileDragTarget::Directory(target_directory) => {
                self.move_dragged_files(file_drag.sources, target_directory)
            }
            FileDragTarget::SidebarBookmarkSlot(slot) => {
                self.add_dragged_sidebar_bookmark(slot, file_drag.sources)
            }
        }
    }

    pub(crate) fn start_file_drag(
        &mut self,
        pressed_path: PathBuf,
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
            target: None,
            phase: FileDragPhase::WaitingForMovement {
                origin: self.cursor_position,
            },
            native_dnd: FileDragNativeDndState::NotRequested,
            column_directories_snapshot,
        });
    }

    fn finish_stationary_file_drag(&mut self, file_drag: FileDragState) {
        if file_drag.sources.len() > 1
            && file_drag
                .sources
                .iter()
                .any(|source| source == &file_drag.pressed_path)
        {
            self.select_path(file_drag.pressed_path);
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

    pub(crate) fn file_drag_release_directory_at_position(
        &self,
        position: iced::Point,
    ) -> Option<PathBuf> {
        for entry_bounds in self.file_entry_bounds.iter().rev() {
            if !entry_bounds.bounds.contains(position) {
                continue;
            }
            return self
                .file_drag_release_directory_for_entry(entry_bounds.pane_id, &entry_bounds.path);
        }

        let pane_id = self.pane_id_at_position(position)?;
        self.pane_view(pane_id)
            .filter(|pane| !pane.is_trash_view)
            .map(|pane| pane.current_dir.clone())
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
        let transfers = paths::transfer_targets(&target_directory, &sources, PasteTargetMode::Move)
            .into_iter()
            .filter(|(source, target)| source != target && !target.starts_with(source))
            .map(|(source, target)| QueuedTransfer::new(source, target))
            .collect::<Vec<_>>();

        if transfers.is_empty() {
            return Task::none();
        }

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

fn file_drag_directory_target_needs_fallback(sources: &[PathBuf], target: &Path) -> bool {
    sources.iter().any(|source| {
        source == target
            || target.starts_with(source)
            || source.parent().is_some_and(|parent| parent == target)
    })
}
