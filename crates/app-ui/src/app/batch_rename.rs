use iced::Task;
use std::collections::HashSet;
use std::path::PathBuf;

use super::FileBrowser;
use crate::model::{same_parent, BatchRenameMessage, BatchRenameSource, BatchRenameState, Message};
use crate::operation_queue::QueuedFileOperation;
use crate::view::batch_rename_preview_name_input_id;

impl FileBrowser {
    pub(super) fn handle_batch_rename_message(
        &mut self,
        message: BatchRenameMessage,
    ) -> Task<Message> {
        match message {
            BatchRenameMessage::OpenSelected => self.open_batch_rename_selected(),
            BatchRenameMessage::Apply => self.apply_batch_rename(),
            BatchRenameMessage::Cancel => {
                self.batch_rename = None;
                Task::none()
            }
            BatchRenameMessage::PreviewNameEditStarted(source) => {
                self.start_batch_rename_preview_name_edit(source)
            }
            message => self.update_batch_rename(|state| state.apply_update(message)),
        }
    }

    pub(super) fn batch_rename_available_for_selection(&self) -> bool {
        let paths = self.selected_paths_for_operation();
        !self.is_trash_view && paths.len() > 1 && same_parent(&paths)
    }

    fn open_batch_rename_selected(&mut self) -> Task<Message> {
        if !self.batch_rename_available_for_selection() {
            return Task::none();
        }
        let paths = self.selected_paths_for_operation();
        self.context_menu = None;
        self.open_with = None;
        self.archive_creation = None;
        self.archive_extraction = None;
        self.renaming = None;
        let _ = self.cancel_address_editing();
        let existing_paths = self.batch_rename_existing_paths(&paths);
        let sources = self.batch_rename_sources_for_paths(&paths);
        self.batch_rename = BatchRenameState::new_with_existing_sources(sources, existing_paths);
        Task::none()
    }

    fn update_batch_rename(&mut self, update: impl FnOnce(&mut BatchRenameState)) -> Task<Message> {
        if let Some(state) = &mut self.batch_rename {
            update(state);
            state.rebuild_preview();
        }
        Task::none()
    }

    pub(super) fn finish_batch_rename_preview_drag(&mut self) {
        if let Some(state) = &mut self.batch_rename {
            state.finish_preview_drag();
        }
    }

    fn apply_batch_rename(&mut self) -> Task<Message> {
        let Some(items) = self.batch_rename.as_ref().and_then(BatchRenameState::plan) else {
            return Task::none();
        };
        self.batch_rename = None;
        self.enqueue_file_operation(QueuedFileOperation::BatchRename { items })
    }

    fn start_batch_rename_preview_name_edit(&mut self, source: PathBuf) -> Task<Message> {
        let has_row = self
            .batch_rename
            .as_ref()
            .and_then(|state| state.preview_target_name_for_source(&source))
            .is_some();
        let update = self.update_batch_rename(|state| {
            state.apply_update(BatchRenameMessage::PreviewNameEditStarted(source.clone()))
        });
        if !has_row {
            return update;
        }

        let input_id = batch_rename_preview_name_input_id(&source);
        Task::batch([
            update,
            iced::widget::operation::focus(input_id.clone()),
            iced::widget::operation::select_all(input_id),
        ])
    }

    fn batch_rename_existing_paths(&self, selected_paths: &[PathBuf]) -> HashSet<PathBuf> {
        let Some(parent) = selected_paths.first().and_then(|path| path.parent()) else {
            return HashSet::new();
        };

        crate::visible_entries::visible_entry_paths(&self.entries, &self.expanded_directories)
            .into_iter()
            .filter(|path| path.parent() == Some(parent))
            .collect()
    }

    fn batch_rename_sources_for_paths(&self, selected_paths: &[PathBuf]) -> Vec<BatchRenameSource> {
        let selected = selected_paths.iter().cloned().collect::<HashSet<_>>();
        crate::visible_entries::visible_entries(&self.entries, &self.expanded_directories)
            .into_iter()
            .filter(|visible| selected.contains(&visible.entry.path))
            .map(|visible| BatchRenameSource::from_entry(visible.entry))
            .collect()
    }
}
