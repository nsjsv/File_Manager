use iced::Element;

use super::FileBrowser;
use crate::model::{Message, ScrollbarRegion};
use crate::view::view_preview_window;

impl FileBrowser {
    pub(super) fn preview_window_content(
        &self,
        preview_bottom_controls_opacity: f32,
    ) -> Element<'_, Message> {
        view_preview_window(
            self.preview.as_ref(),
            self.text_preview_document.as_ref(),
            self.preview_size,
            self.audio_preview.as_ref(),
            self.video_preview.as_ref(),
            preview_bottom_controls_opacity,
            self.operation_progress_animation_frame,
            self.scrollbar_visibility_for(&ScrollbarRegion::PreviewDirectory),
            self.scrollbar_viewport_for(&ScrollbarRegion::PreviewDirectory),
            self.scrollbar_visibility_for(&ScrollbarRegion::PreviewArchive),
            self.scrollbar_viewport_for(&ScrollbarRegion::PreviewArchive),
            self.scrollbar_visibility_for(&ScrollbarRegion::PreviewDocument),
            self.scrollbar_viewport_for(&ScrollbarRegion::PreviewDocument),
            self.scrollbar_visibility_for(&ScrollbarRegion::MarkdownPreview),
            self.scrollbar_viewport_for(&ScrollbarRegion::MarkdownPreview),
        )
    }
}
