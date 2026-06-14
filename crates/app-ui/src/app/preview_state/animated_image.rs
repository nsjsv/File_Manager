use std::path::PathBuf;
use std::time::Duration;

use iced::Task;

use crate::animated_image_preview::AnimatedImagePreview;
use crate::app::FileBrowser;
use crate::model::{Message, PreviewContent, PreviewState};

impl FileBrowser {
    pub(in crate::app) fn accept_animated_image_preview(
        &mut self,
        path: PathBuf,
        preview_outcome: Result<AnimatedImagePreview, String>,
    ) -> Task<Message> {
        if !matches!(
            &self.preview,
            Some(PreviewState::Loading(loading_path)) if loading_path == &path
        ) {
            return Task::none();
        }

        match preview_outcome {
            Ok(preview) => {
                let width = preview.width();
                let height = preview.height();
                self.text_preview_document = None;
                self.clear_audio_preview();
                self.clear_video_preview();
                self.preview = Some(PreviewState::Ready(PreviewContent::AnimatedImage(preview)));
                self.error = None;
                self.open_image_preview_window_for_dimensions(width, height)
            }
            Err(error) => {
                self.text_preview_document = None;
                self.clear_audio_preview();
                self.clear_video_preview();
                self.preview = Some(PreviewState::Error(error));
                self.open_image_preview_error_window()
            }
        }
    }

    pub(in crate::app) fn active_animated_image_preview_frame_delay(
        &self,
    ) -> Option<(PathBuf, Duration)> {
        let PreviewState::Ready(PreviewContent::AnimatedImage(preview)) = self.preview.as_ref()?
        else {
            return None;
        };

        preview
            .current_frame_delay()
            .map(|delay| (preview.path().to_path_buf(), delay))
    }

    pub(in crate::app) fn advance_animated_image_preview(
        &mut self,
        path: PathBuf,
    ) -> Task<Message> {
        let Some(PreviewState::Ready(PreviewContent::AnimatedImage(preview))) =
            self.preview.as_mut()
        else {
            return Task::none();
        };
        if preview.path() != path.as_path() {
            return Task::none();
        }

        preview.advance_frame();
        Task::none()
    }
}
