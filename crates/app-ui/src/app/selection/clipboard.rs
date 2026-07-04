use std::path::PathBuf;

use desktop_linux::{
    ClipboardImage, DesktopClipboardContent, FileClipboardOperation, FileClipboardSelection,
    WaylandDndDropOrigin, WaylandDndDropPosition, WaylandDndFileDrop,
};
use iced::{Point, Task};

use crate::app::paths::{self, PasteTargetMode};
use crate::app::FileBrowser;
use crate::commands::{
    create_clipboard_file_command, read_desktop_clipboard_command, write_file_clipboard_command,
};
use crate::model::{
    ContextMenuState, DestructiveActionConfirmation, FileDropPrompt, Message, PendingOperation,
    TransferConflictMode,
};
use crate::operation_queue::{QueuedFileOperation, QueuedTransfer};

impl FileBrowser {
    pub(in crate::app) fn copy_selected(&mut self) -> Task<Message> {
        self.context_menu = None;
        if self.is_trash_view {
            return Task::none();
        }
        let paths = self.selected_paths_for_operation();
        if paths.is_empty() {
            return Task::none();
        }
        self.pending_operation = Some(PendingOperation::Copy(paths.clone()));
        write_file_clipboard_command(FileClipboardSelection::new(
            FileClipboardOperation::Copy,
            paths,
        ))
    }

    pub(in crate::app) fn move_selected(&mut self) -> Task<Message> {
        self.context_menu = None;
        if self.is_trash_view {
            return Task::none();
        }
        let paths = self.selected_paths_for_operation();
        if paths.is_empty() {
            return Task::none();
        }
        self.pending_operation = Some(PendingOperation::Move(paths.clone()));
        write_file_clipboard_command(FileClipboardSelection::new(
            FileClipboardOperation::Move,
            paths,
        ))
    }

    pub(in crate::app) fn trash_selected(&mut self) -> Task<Message> {
        self.context_menu = None;
        if self.is_trash_view {
            return self.delete_selected_trash_entries();
        }
        let paths = self.selected_paths_for_operation();
        if paths.is_empty() {
            return Task::none();
        }
        let (network_paths, local_paths): (Vec<_>, Vec<_>) = paths
            .into_iter()
            .partition(|path| self.path_is_mounted_network(path));
        match (network_paths.is_empty(), local_paths.is_empty()) {
            (true, false) => {
                self.enqueue_file_operation(QueuedFileOperation::Trash { paths: local_paths })
            }
            (false, true) => {
                self.request_destructive_action_confirmation(
                    DestructiveActionConfirmation::DeletePermanently {
                        paths: network_paths,
                    },
                );
                Task::none()
            }
            (false, false) => {
                self.show_global_error(
                    "Delete local and network items separately so local files can use Trash"
                        .to_owned(),
                );
                Task::none()
            }
            (true, true) => Task::none(),
        }
    }

    pub(in crate::app) fn restore_selected(&mut self) -> Task<Message> {
        self.context_menu = None;
        if !self.is_trash_view {
            return Task::none();
        }

        let entries = self.selected_trash_entries_for_operation();
        if entries.is_empty() {
            Task::none()
        } else {
            self.enqueue_file_operation(QueuedFileOperation::Restore { entries })
        }
    }

    fn delete_selected_trash_entries(&mut self) -> Task<Message> {
        let entries = self.selected_trash_entries_for_operation();
        if entries.is_empty() {
            Task::none()
        } else {
            self.request_destructive_action_confirmation(
                DestructiveActionConfirmation::DeleteTrashEntries { entries },
            );
            Task::none()
        }
    }

    pub(in crate::app) fn empty_trash_requested(&mut self) -> Task<Message> {
        self.context_menu = None;
        if !self.is_trash_view || self.trash_entries.is_empty() {
            return Task::none();
        }
        self.request_destructive_action_confirmation(DestructiveActionConfirmation::EmptyTrash);
        Task::none()
    }

    pub(in crate::app) fn confirm_destructive_action(&mut self) -> Task<Message> {
        let Some(confirmation) = self.destructive_action_confirmation.take() else {
            return Task::none();
        };

        match confirmation {
            DestructiveActionConfirmation::DeleteTrashEntries { entries } => {
                if entries.is_empty() {
                    Task::none()
                } else {
                    self.enqueue_file_operation(QueuedFileOperation::DeleteTrashEntries { entries })
                }
            }
            DestructiveActionConfirmation::DeletePermanently { paths } => {
                if paths.is_empty() {
                    Task::none()
                } else {
                    self.enqueue_file_operation(QueuedFileOperation::DeletePermanently { paths })
                }
            }
            DestructiveActionConfirmation::EmptyTrash => {
                self.enqueue_file_operation(QueuedFileOperation::EmptyTrash)
            }
        }
    }

    pub(in crate::app) fn cancel_destructive_action(&mut self) -> Task<Message> {
        self.destructive_action_confirmation = None;
        Task::none()
    }

    fn request_destructive_action_confirmation(
        &mut self,
        confirmation: DestructiveActionConfirmation,
    ) {
        self.destructive_action_confirmation = Some(confirmation);
        self.transfer_conflict = None;
        self.context_menu = None;
        self.operation_queue.close_panel();
    }

    pub(in crate::app) fn create_directory_in(&mut self, directory: PathBuf) -> Task<Message> {
        self.context_menu = None;
        if self.is_trash_view {
            return Task::none();
        }
        self.clear_preview();
        self.renaming = None;
        self.drag_selection_anchor = None;
        self.file_drag = None;
        self.enqueue_file_operation(QueuedFileOperation::CreateDirectory { parent: directory })
    }

    pub(in crate::app) fn create_empty_file_in(&mut self, directory: PathBuf) -> Task<Message> {
        self.context_menu = None;
        if self.is_trash_view {
            return Task::none();
        }
        self.clear_preview();
        self.renaming = None;
        self.drag_selection_anchor = None;
        self.file_drag = None;
        self.enqueue_file_operation(QueuedFileOperation::CreateEmptyFile { parent: directory })
    }

    pub(in crate::app) fn paste_pending(&mut self) -> Task<Message> {
        if self.is_trash_view {
            self.context_menu = None;
            return Task::none();
        }
        let paste_directory = self.paste_target_directory();
        self.context_menu = None;
        read_desktop_clipboard_command(paste_directory, self.pending_operation.clone())
    }

    pub(in crate::app) fn accept_file_clipboard_write(
        &mut self,
        result: Result<(), String>,
    ) -> Task<Message> {
        match result {
            Ok(()) => self.clear_global_error(),
            Err(error) => self.show_global_error(error),
        }
        Task::none()
    }

    pub(in crate::app) fn accept_desktop_clipboard_paste(
        &mut self,
        paste_directory: PathBuf,
        fallback_operation: Option<PendingOperation>,
        content: Result<Option<DesktopClipboardContent>, String>,
    ) -> Task<Message> {
        match content {
            Ok(Some(content)) => self.paste_desktop_clipboard_content(paste_directory, content),
            Ok(None) => self.paste_optional_operation(paste_directory, fallback_operation),
            Err(error) => {
                if fallback_operation.is_some() {
                    self.paste_optional_operation(paste_directory, fallback_operation)
                } else {
                    self.show_global_error(error);
                    Task::none()
                }
            }
        }
    }

    pub(in crate::app) fn accept_clipboard_file_created(
        &mut self,
        result: Result<PathBuf, String>,
    ) -> Task<Message> {
        match result {
            Ok(path) => {
                self.invalidate_list_directory_summary_subtree_and_ancestor_chain(&path);
                self.reload_current_preserving_list_directory_summaries()
            }
            Err(error) => {
                self.show_global_error(error);
                Task::none()
            }
        }
    }

    pub(in crate::app) fn accept_wayland_file_drop(
        &mut self,
        result: Result<WaylandDndFileDrop, String>,
    ) -> Task<Message> {
        if self.is_trash_view {
            return Task::none();
        }
        match result {
            Ok(drop) => match drop.origin {
                WaylandDndDropOrigin::External => self.accept_external_wayland_file_drop(drop),
                WaylandDndDropOrigin::Internal => self.accept_internal_wayland_file_drop(drop),
            },
            Err(error) => {
                self.show_global_error(error);
                Task::none()
            }
        }
    }

    fn accept_external_wayland_file_drop(&mut self, drop: WaylandDndFileDrop) -> Task<Message> {
        let position = wayland_drop_position(drop.position);
        if let Some(position) = position {
            self.cursor_position = position;
        }
        let paste_directory = self.paste_target_directory_for_wayland_drop(position);
        self.request_file_drop_prompt(paste_directory, drop.selection.paths)
    }

    fn accept_internal_wayland_file_drop(&mut self, drop: WaylandDndFileDrop) -> Task<Message> {
        if !self.active_file_drag_matches_wayland_drop(&drop.selection.paths) {
            return Task::none();
        }

        let position = wayland_drop_position(drop.position);
        let release_directory = position.and_then(|position| {
            self.cursor_position = position;
            self.refresh_file_drag_target_at_position(position);
            self.file_drag_release_directory_at_position(position)
        });

        Task::batch([
            self.finish_drag_selection(release_directory),
            self.schedule_thumbnail_refresh(),
        ])
    }

    fn active_file_drag_matches_wayland_drop(&self, paths: &[PathBuf]) -> bool {
        self.file_drag
            .as_ref()
            .is_some_and(|file_drag| file_drag.is_dragging() && file_drag.sources == paths)
    }

    fn refresh_file_drag_target_at_position(&mut self, position: Point) {
        if position.x <= self.sidebar_width {
            drop(self.update_sidebar_bookmark_drop_slot(position));
        } else {
            drop(self.clear_sidebar_bookmark_drop_slot());
        }
    }

    fn request_file_drop_prompt(
        &mut self,
        paste_directory: PathBuf,
        paths: Vec<PathBuf>,
    ) -> Task<Message> {
        if paths.is_empty() {
            return Task::none();
        }
        if self.destructive_action_confirmation.is_some()
            || self.file_drop_prompt.is_some()
            || self.transfer_conflict.is_some()
        {
            self.show_global_error(
                "Finish the current file operation prompt before dropping files".to_owned(),
            );
            return Task::none();
        }
        self.context_menu = None;
        self.open_with = None;
        self.operation_queue.close_panel();
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        self.file_drop_prompt = Some(FileDropPrompt {
            paste_directory,
            paths,
        });
        Task::none()
    }

    pub(in crate::app) fn apply_file_drop_operation(
        &mut self,
        operation: FileClipboardOperation,
    ) -> Task<Message> {
        let Some(prompt) = self.file_drop_prompt.take() else {
            return Task::none();
        };
        self.paste_file_clipboard_selection(
            prompt.paste_directory,
            FileClipboardSelection::new(operation, prompt.paths),
        )
    }

    pub(in crate::app) fn cancel_file_drop(&mut self) -> Task<Message> {
        self.file_drop_prompt = None;
        Task::none()
    }

    fn paste_desktop_clipboard_content(
        &mut self,
        paste_directory: PathBuf,
        content: DesktopClipboardContent,
    ) -> Task<Message> {
        match content {
            DesktopClipboardContent::Files(selection) => {
                self.paste_file_clipboard_selection(paste_directory, selection)
            }
            DesktopClipboardContent::Text(text) => {
                self.create_clipboard_text_file(paste_directory, text)
            }
            DesktopClipboardContent::Image(image) => {
                self.create_clipboard_image_file(paste_directory, image)
            }
        }
    }

    fn paste_file_clipboard_selection(
        &mut self,
        paste_directory: PathBuf,
        selection: FileClipboardSelection,
    ) -> Task<Message> {
        let operation = match selection.operation {
            FileClipboardOperation::Copy => PendingOperation::Copy(selection.paths),
            FileClipboardOperation::Move => PendingOperation::Move(selection.paths),
        };
        self.paste_operation(paste_directory, operation)
    }

    fn create_clipboard_text_file(
        &mut self,
        paste_directory: PathBuf,
        text: String,
    ) -> Task<Message> {
        self.context_menu = None;
        let target = paste_directory.join("Pasted Text.txt");
        create_clipboard_file_command(target, text.into_bytes())
    }

    fn create_clipboard_image_file(
        &mut self,
        paste_directory: PathBuf,
        image: ClipboardImage,
    ) -> Task<Message> {
        self.context_menu = None;
        let target = paste_directory.join(format!("Screenshot.{}", image.extension));
        create_clipboard_file_command(target, image.bytes)
    }

    fn paste_optional_operation(
        &mut self,
        paste_directory: PathBuf,
        operation: Option<PendingOperation>,
    ) -> Task<Message> {
        let Some(operation) = operation else {
            return Task::none();
        };
        self.paste_operation(paste_directory, operation)
    }

    fn paste_operation(
        &mut self,
        paste_directory: PathBuf,
        operation: PendingOperation,
    ) -> Task<Message> {
        let (mode, transfers) = match operation {
            PendingOperation::Copy(sources) => {
                let transfers =
                    paths::transfer_targets(&paste_directory, &sources, PasteTargetMode::Copy)
                        .into_iter()
                        .map(|(source, target)| QueuedTransfer::new(source, target))
                        .collect::<Vec<_>>();
                (TransferConflictMode::Copy, transfers)
            }
            PendingOperation::Move(sources) => {
                let transfers =
                    paths::transfer_targets(&paste_directory, &sources, PasteTargetMode::Move)
                        .into_iter()
                        .filter(|(source, target)| source != target)
                        .map(|(source, target)| QueuedTransfer::new(source, target))
                        .collect::<Vec<_>>();
                self.pending_operation = None;
                (TransferConflictMode::Move, transfers)
            }
        };

        if transfers.is_empty() {
            return Task::none();
        }

        self.enqueue_or_confirm_transfers(mode, transfers)
    }

    fn paste_target_directory(&self) -> PathBuf {
        self.context_menu
            .as_ref()
            .and_then(ContextMenuState::paste_directory)
            .cloned()
            .or_else(|| self.cursor_paste_directory.clone())
            .unwrap_or_else(|| self.current_dir.clone())
    }

    fn paste_target_directory_for_wayland_drop(&self, position: Option<Point>) -> PathBuf {
        if let Some(position) = position {
            for entry_bounds in self.file_entry_bounds.iter().rev() {
                if entry_bounds.bounds.contains(position) {
                    if let Some(directory) = self.cursor_paste_directory_for_entry_in_pane(
                        entry_bounds.pane_id,
                        &entry_bounds.path,
                    ) {
                        return directory;
                    }
                }
            }

            if let Some(directory) = self
                .pane_id_at_position(position)
                .and_then(|pane_id| self.pane_view(pane_id))
                .filter(|pane| !pane.is_trash_view)
                .map(|pane| pane.current_dir.clone())
            {
                return directory;
            }
        }

        self.paste_target_directory()
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
        NetworkConnection, NetworkConnectionId, NetworkMountState, NetworkProtocol,
    };
    use file_core::{DirectoryEntry, EntryMetadata, FileKind};
    use iced::futures::StreamExt;
    use iced::Task;
    use iced_runtime::Action;

    use super::*;
    use crate::config;
    use crate::model::{
        FileDragNativeDndState, FileDragPhase, FileDragState, FileDragTarget, TransferConflictItem,
    };

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

    fn mount_network_connection(browser: &mut FileBrowser, mount_path: PathBuf) {
        let connection = NetworkConnection::new(
            NetworkConnectionId::new("nas"),
            "NAS",
            NetworkProtocol::Smb,
            "smb://server/share",
        )
        .unwrap();
        let id = connection.id.clone();
        browser.network_connections =
            crate::network_connections::NetworkConnectionState::from_connections(vec![connection]);
        browser
            .network_connections
            .accept_loaded(vec![(id, NetworkMountState::Mounted(mount_path))]);
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

        drop(browser.accept_wayland_file_drop(Ok(wayland_file_drop(
            FileClipboardOperation::Copy,
            vec![source.clone()],
            WaylandDndDropOrigin::External,
        ))));

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
    fn second_wayland_file_drop_keeps_pending_prompt() {
        let first_source = PathBuf::from("/outside/first.txt");
        let second_source = PathBuf::from("/outside/second.txt");
        let mut browser = browser_with_entries(&[]);

        drop(browser.accept_wayland_file_drop(Ok(wayland_file_drop(
            FileClipboardOperation::Move,
            vec![first_source.clone()],
            WaylandDndDropOrigin::External,
        ))));
        drop(browser.accept_wayland_file_drop(Ok(wayland_file_drop(
            FileClipboardOperation::Move,
            vec![second_source],
            WaylandDndDropOrigin::External,
        ))));

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

        drop(browser.accept_wayland_file_drop(Ok(wayland_file_drop(
            FileClipboardOperation::Move,
            vec![source.clone()],
            WaylandDndDropOrigin::External,
        ))));

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
    async fn internal_wayland_file_drop_finishes_active_file_drag() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let destination = temp_dir.path().join("destination");
        let source = temp_dir.path().join("report.txt");
        fs::create_dir_all(&destination).expect("create destination");
        fs::write(&source, b"report").expect("write source");
        let mut browser = browser_with_entries(&[source.clone()]);
        browser.file_drag = Some(FileDragState {
            sources: vec![source.clone()],
            pressed_path: source.clone(),
            target: Some(FileDragTarget::Directory(destination.clone())),
            phase: FileDragPhase::Dragging,
            native_dnd: FileDragNativeDndState::NotRequested,
            column_directories_snapshot: Vec::new(),
        });

        let (mode, transfers, conflicts) = transfer_conflict_check_message(
            browser.accept_wayland_file_drop(Ok(wayland_file_drop(
                FileClipboardOperation::Move,
                vec![source.clone()],
                WaylandDndDropOrigin::Internal,
            ))),
        )
        .await;

        assert!(browser.file_drop_prompt.is_none());
        assert!(browser.file_drag.is_none());
        assert_eq!(mode, TransferConflictMode::Move);
        assert_eq!(
            transfers,
            vec![QueuedTransfer::new(source, destination.join("report.txt"))]
        );
        assert!(conflicts.is_empty());
    }

    #[test]
    fn cancelled_file_drop_clears_prompt() {
        let source = PathBuf::from("/outside/report.txt");
        let mut browser = browser_with_entries(&[]);

        drop(browser.accept_wayland_file_drop(Ok(wayland_file_drop(
            FileClipboardOperation::Move,
            vec![source],
            WaylandDndDropOrigin::External,
        ))));
        drop(browser.cancel_file_drop());

        assert!(browser.file_drop_prompt.is_none());
    }

    #[test]
    fn local_delete_still_uses_trash_operation() {
        let local_path = PathBuf::from("/workspace/local.txt");
        let mut browser = browser_with_entries(std::slice::from_ref(&local_path));

        let command = browser.trash_selected();
        drop(command);

        assert!(browser.destructive_action_confirmation.is_none());
        assert_eq!(browser.operation_queue.tasks().len(), 1);
        assert!(matches!(
            &browser.operation_queue.tasks()[0].operation,
            QueuedFileOperation::Trash { paths } if paths == &vec![local_path]
        ));
    }

    #[test]
    fn network_delete_requests_permanent_delete_confirmation() {
        let mount_path = PathBuf::from("/run/user/1000/gvfs/smb-share:server=server,share=share");
        let network_path = mount_path.join("remote.txt");
        let mut browser = browser_with_entries(std::slice::from_ref(&network_path));
        mount_network_connection(&mut browser, mount_path);

        let command = browser.trash_selected();
        drop(command);

        assert_eq!(browser.operation_queue.tasks().len(), 0);
        assert!(matches!(
            &browser.destructive_action_confirmation,
            Some(DestructiveActionConfirmation::DeletePermanently { paths })
                if paths == &vec![network_path]
        ));
    }

    #[test]
    fn mixed_local_and_network_delete_is_rejected() {
        let mount_path = PathBuf::from("/run/user/1000/gvfs/smb-share:server=server,share=share");
        let network_path = mount_path.join("remote.txt");
        let local_path = PathBuf::from("/workspace/local.txt");
        let mut browser = browser_with_entries(&[local_path, network_path]);
        mount_network_connection(&mut browser, mount_path);

        let command = browser.trash_selected();
        drop(command);

        assert_eq!(browser.operation_queue.tasks().len(), 0);
        assert!(browser.destructive_action_confirmation.is_none());
        assert_eq!(
            browser.error.as_deref(),
            Some("Delete local and network items separately so local files can use Trash")
        );
    }
}
