use std::path::PathBuf;

use iced::Command;

use super::FileBrowser;
use crate::model::{Message, PreviewContent, PreviewState};

impl FileBrowser {
    pub(super) fn accept_preview(
        &mut self,
        path: PathBuf,
        preview_outcome: Result<PreviewContent, String>,
    ) -> Command<Message> {
        let is_active_preview_request = matches!(
            &self.preview,
            Some(PreviewState::Loading(loading_path)) if loading_path == &path
        );
        if !is_active_preview_request {
            return Command::none();
        }

        match preview_outcome {
            Ok(preview) => {
                self.preview = Some(PreviewState::Ready(preview));
                self.error = None;
            }
            Err(error) => {
                self.preview = Some(PreviewState::Error(error));
            }
        }

        Command::none()
    }

    pub(super) fn toggle_archive_preview_directory(&mut self, entry_id: usize) -> Command<Message> {
        let Some(PreviewState::Ready(PreviewContent::Archive { entries, .. })) = &mut self.preview
        else {
            return Command::none();
        };
        let Some(entry) = entries.get_mut(entry_id) else {
            return Command::none();
        };
        if entry.is_directory() {
            entry.is_expanded = !entry.is_expanded;
        }

        Command::none()
    }
}
