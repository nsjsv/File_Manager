use std::path::{Path, PathBuf};

use iced::widget::text_editor;

pub(crate) const TEXT_PREVIEW_LINE_LIMIT: usize = 10_000;
pub(crate) const TEXT_PREVIEW_INITIAL_LINE_LIMIT: usize = 50;
pub(crate) const TEXT_PREVIEW_CHUNK_LINE_LIMIT: usize = 500;
pub(crate) const TEXT_PREVIEW_LOAD_MORE_THRESHOLD: usize = 10;
pub(crate) const TEXT_PREVIEW_TEXT_SIZE: f32 = 16.0;
pub(crate) const TEXT_PREVIEW_LINE_HEIGHT: f32 = 1.3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextPreviewFormat {
    Plain,
    Markdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkdownPreviewMode {
    Rendered,
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextPreviewLineLimitNotice {
    total_line_count: usize,
}

impl TextPreviewLineLimitNotice {
    pub(crate) fn for_total_line_count(total_line_count: usize) -> Option<Self> {
        (total_line_count > TEXT_PREVIEW_LINE_LIMIT).then_some(Self { total_line_count })
    }

    pub(crate) fn total_line_count(self) -> usize {
        self.total_line_count
    }

    pub(crate) fn preview_line_limit(self) -> usize {
        TEXT_PREVIEW_LINE_LIMIT
    }

    pub(crate) fn label(self) -> String {
        format!(
            "Only showing {} lines. Full line count: {}.",
            self.preview_line_limit(),
            self.total_line_count()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextPreviewChunkRequest {
    pub(crate) path: PathBuf,
    pub(crate) generation: u64,
    pub(crate) start_offset: u64,
    pub(crate) loaded_line_count: usize,
    pub(crate) line_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextPreviewChunk {
    pub(crate) start_offset: u64,
    pub(crate) content: String,
    pub(crate) line_count: usize,
    pub(crate) next_offset: Option<u64>,
    pub(crate) line_limit_notice: Option<TextPreviewLineLimitNotice>,
}

pub(crate) struct TextPreviewDocument {
    path: PathBuf,
    content: text_editor::Content,
    markdown_preview_mode: MarkdownPreviewMode,
    generation: u64,
    next_offset: Option<u64>,
    loading_offset: Option<u64>,
    loaded_line_count: usize,
    line_limit_notice: Option<TextPreviewLineLimitNotice>,
    chunk_error: Option<String>,
    scroll_top_line: i32,
    visual_scroll_line_offset: i32,
    is_scrolled_to_preview_end: bool,
    content_revision: u64,
}

impl TextPreviewDocument {
    pub(crate) fn new_initial(
        path: PathBuf,
        content: &str,
        format: TextPreviewFormat,
        generation: u64,
        next_offset: Option<u64>,
        loaded_line_count: usize,
        line_limit_notice: Option<TextPreviewLineLimitNotice>,
    ) -> Self {
        let content_state = text_editor::Content::with_text(content);

        Self {
            path,
            content: content_state,
            markdown_preview_mode: initial_markdown_preview_mode(format),
            generation,
            next_offset,
            loading_offset: None,
            loaded_line_count,
            line_limit_notice,
            chunk_error: None,
            scroll_top_line: 0,
            visual_scroll_line_offset: 0,
            is_scrolled_to_preview_end: false,
            content_revision: 0,
        }
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        self.path.as_path()
    }

    pub(crate) fn content(&self) -> &text_editor::Content {
        &self.content
    }

    pub(crate) fn content_text(&self) -> String {
        self.content.text()
    }

    pub(crate) fn line_count(&self) -> usize {
        self.content.line_count().max(1)
    }

    pub(crate) fn line_number_digit_count(&self) -> usize {
        self.line_count().to_string().len()
    }

    pub(crate) fn visual_scroll_line_offset(&self) -> usize {
        self.visual_scroll_line_offset.max(0) as usize
    }

    pub(crate) fn content_revision(&self) -> u64 {
        self.content_revision
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn next_offset(&self) -> Option<u64> {
        self.next_offset
    }

    pub(crate) fn loaded_line_count(&self) -> usize {
        self.loaded_line_count
    }

    pub(crate) fn line_limit_notice(&self) -> Option<TextPreviewLineLimitNotice> {
        self.line_limit_notice
    }

    pub(crate) fn chunk_error(&self) -> Option<&str> {
        self.chunk_error.as_deref()
    }

    pub(crate) fn markdown_preview_mode(&self) -> MarkdownPreviewMode {
        self.markdown_preview_mode
    }

    pub(crate) fn select_markdown_preview_mode(&mut self, mode: MarkdownPreviewMode) {
        self.markdown_preview_mode = mode;
    }

    pub(crate) fn is_scrolled_to_preview_end(&self) -> bool {
        self.is_scrolled_to_preview_end
    }

    pub(crate) fn perform(
        &mut self,
        action: text_editor::Action,
        viewport_height: f32,
    ) -> Option<TextPreviewChunkRequest> {
        if action.is_edit() {
            return None;
        }

        self.update_preview_end_visibility(&action, viewport_height);
        self.content.perform(action);
        self.request_next_chunk_for_text_view(viewport_height)
    }

    pub(crate) fn request_next_chunk_for_rendered_markdown(
        &mut self,
        offset_y: f32,
        viewport_height: f32,
        content_height: f32,
    ) -> Option<TextPreviewChunkRequest> {
        if !offset_y.is_finite() || !viewport_height.is_finite() || !content_height.is_finite() {
            return None;
        }
        if content_height <= viewport_height {
            return None;
        }

        let remaining_height = (content_height - viewport_height - offset_y).max(0.0);
        let threshold_height = TEXT_PREVIEW_LOAD_MORE_THRESHOLD as f32
            * TEXT_PREVIEW_TEXT_SIZE
            * TEXT_PREVIEW_LINE_HEIGHT;
        if remaining_height > threshold_height {
            return None;
        }

        self.request_next_chunk()
    }

    pub(crate) fn append_chunk(&mut self, chunk: TextPreviewChunk, viewport_height: f32) -> bool {
        if self.loading_offset != Some(chunk.start_offset) {
            return false;
        }

        if !chunk.content.is_empty() {
            self.append_chunk_text_preserving_editor_viewport(&chunk.content);
        }

        self.loaded_line_count = self
            .loaded_line_count
            .saturating_add(chunk.line_count)
            .min(TEXT_PREVIEW_LINE_LIMIT);
        self.next_offset = if self.loaded_line_count >= TEXT_PREVIEW_LINE_LIMIT {
            None
        } else {
            chunk.next_offset
        };
        self.loading_offset = None;
        self.line_limit_notice = chunk.line_limit_notice.or(self.line_limit_notice);
        self.chunk_error = None;
        self.update_preview_end_after_content_change(viewport_height);
        true
    }

    pub(crate) fn accept_chunk_error(&mut self, start_offset: u64, error: String) -> bool {
        if self.loading_offset != Some(start_offset) {
            return false;
        }

        self.loading_offset = None;
        self.chunk_error = Some(error);
        true
    }

    fn request_next_chunk_for_text_view(
        &mut self,
        viewport_height: f32,
    ) -> Option<TextPreviewChunkRequest> {
        let visible_lines = visible_text_preview_lines(viewport_height);
        let last_visible_line =
            (self.scroll_top_line.max(0) as usize).saturating_add(visible_lines);
        if self.loaded_line_count.saturating_sub(last_visible_line)
            > TEXT_PREVIEW_LOAD_MORE_THRESHOLD
        {
            return None;
        }

        self.request_next_chunk()
    }

    fn request_next_chunk(&mut self) -> Option<TextPreviewChunkRequest> {
        let start_offset = self.next_offset?;
        if self.loading_offset.is_some() || self.loaded_line_count >= TEXT_PREVIEW_LINE_LIMIT {
            return None;
        }

        let remaining_line_limit = TEXT_PREVIEW_LINE_LIMIT.saturating_sub(self.loaded_line_count);
        let line_limit = remaining_line_limit.min(TEXT_PREVIEW_CHUNK_LINE_LIMIT);
        if line_limit == 0 {
            return None;
        }

        self.loading_offset = Some(start_offset);
        self.chunk_error = None;
        Some(TextPreviewChunkRequest {
            path: self.path.clone(),
            generation: self.generation,
            start_offset,
            loaded_line_count: self.loaded_line_count,
            line_limit,
        })
    }

    fn append_chunk_text_preserving_editor_viewport(&mut self, chunk_content: &str) {
        let mut content = self.content.text();
        let cursor = self.content.cursor();

        if !content.is_empty() && !content.ends_with('\n') && !content.ends_with('\r') {
            content.push('\n');
        }
        content.push_str(chunk_content);

        self.content = text_editor::Content::with_text(&content);
        self.apply_visual_scroll_to_content();
        self.content.move_to(cursor);
        self.content_revision = self.content_revision.wrapping_add(1);
    }

    fn apply_visual_scroll_to_content(&mut self) {
        let lines = self.visual_scroll_line_offset.max(0);
        if lines == 0 {
            return;
        }

        self.content.perform(text_editor::Action::Scroll { lines });
    }

    fn update_preview_end_visibility(
        &mut self,
        action: &text_editor::Action,
        viewport_height: f32,
    ) {
        let max_scroll_top_line = self.max_scroll_top_line(viewport_height);
        match action {
            text_editor::Action::Scroll { lines } => {
                self.scroll_top_line =
                    (self.scroll_top_line + *lines).clamp(0, max_scroll_top_line);
                self.visual_scroll_line_offset = (self.visual_scroll_line_offset + *lines).max(0);
            }
            text_editor::Action::Move(motion) | text_editor::Action::Select(motion) => {
                let visible_lines = visible_text_preview_lines(viewport_height);
                self.scroll_top_line = scroll_line_after_motion(
                    self.scroll_top_line,
                    *motion,
                    visible_lines,
                    max_scroll_top_line,
                );
                self.visual_scroll_line_offset = scroll_line_after_motion(
                    self.visual_scroll_line_offset,
                    *motion,
                    visible_lines,
                    self.visual_scroll_line_offset.max(max_scroll_top_line),
                );
            }
            _ => {}
        }

        self.is_scrolled_to_preview_end =
            max_scroll_top_line > 0 && self.scroll_top_line >= max_scroll_top_line;
    }

    fn update_preview_end_after_content_change(&mut self, viewport_height: f32) {
        let max_scroll_top_line = self.max_scroll_top_line(viewport_height);
        self.scroll_top_line = self.scroll_top_line.clamp(0, max_scroll_top_line);
        self.is_scrolled_to_preview_end =
            max_scroll_top_line > 0 && self.scroll_top_line >= max_scroll_top_line;
    }

    fn max_scroll_top_line(&self, viewport_height: f32) -> i32 {
        let visible_lines = visible_text_preview_lines(viewport_height);
        self.content
            .line_count()
            .saturating_sub(visible_lines)
            .try_into()
            .unwrap_or(i32::MAX)
    }
}

fn initial_markdown_preview_mode(format: TextPreviewFormat) -> MarkdownPreviewMode {
    match format {
        TextPreviewFormat::Plain => MarkdownPreviewMode::Raw,
        TextPreviewFormat::Markdown => MarkdownPreviewMode::Rendered,
    }
}

fn scroll_line_after_motion(
    current_scroll_top_line: i32,
    motion: text_editor::Motion,
    visible_lines: usize,
    max_scroll_top_line: i32,
) -> i32 {
    let visible_lines = i32::try_from(visible_lines).unwrap_or(i32::MAX);
    match motion {
        text_editor::Motion::DocumentEnd => max_scroll_top_line,
        text_editor::Motion::DocumentStart => 0,
        text_editor::Motion::PageDown => current_scroll_top_line.saturating_add(visible_lines),
        text_editor::Motion::PageUp => current_scroll_top_line.saturating_sub(visible_lines),
        _ => current_scroll_top_line,
    }
    .clamp(0, max_scroll_top_line)
}

fn visible_text_preview_lines(viewport_height: f32) -> usize {
    let line_height = TEXT_PREVIEW_TEXT_SIZE * TEXT_PREVIEW_LINE_HEIGHT;
    if !viewport_height.is_finite() || viewport_height <= line_height {
        return 1;
    }

    (viewport_height / line_height).floor().max(1.0) as usize
}

pub(crate) fn text_preview_loaded_line_count(content: &str) -> usize {
    if content.is_empty() {
        return 0;
    }

    content.bytes().filter(|byte| *byte == b'\n').count() + 1
}

pub(crate) fn render_text_preview(path: &Path, content: &str) -> (String, TextPreviewFormat) {
    let format = text_preview_format_for_path(path);
    (content.to_owned(), format)
}

pub(crate) fn text_preview_format_for_path(path: &Path) -> TextPreviewFormat {
    path.extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| markdown_extension(extension))
        .map(|_| TextPreviewFormat::Markdown)
        .unwrap_or(TextPreviewFormat::Plain)
}

fn markdown_extension(extension: &str) -> bool {
    ["md", "markdown", "mdown", "mkd"]
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn document_with_lines(line_count: usize, next_offset: Option<u64>) -> TextPreviewDocument {
        let content = (0..line_count)
            .map(|line_number| format!("line {line_number}"))
            .collect::<Vec<_>>()
            .join("\n");

        TextPreviewDocument::new_initial(
            PathBuf::from("large.txt"),
            &content,
            TextPreviewFormat::Plain,
            7,
            next_offset,
            line_count,
            None,
        )
    }

    #[test]
    fn text_preview_document_keeps_line_numbers_out_of_selectable_text() {
        let content = "first line\nsecond line\n";
        let document = TextPreviewDocument::new_initial(
            PathBuf::from("note.txt"),
            content,
            TextPreviewFormat::Plain,
            1,
            None,
            text_preview_loaded_line_count(content),
            None,
        );

        assert_eq!(document.content().text(), content);
        assert_eq!(document.line_count(), 3);
    }

    #[test]
    fn empty_text_preview_document_keeps_empty_selectable_text() {
        let document = TextPreviewDocument::new_initial(
            PathBuf::from("empty.txt"),
            "",
            TextPreviewFormat::Plain,
            1,
            None,
            0,
            None,
        );

        assert_eq!(document.content().text(), "");
        assert_eq!(document.line_count(), 1);
    }

    #[test]
    fn text_preview_requests_next_chunk_only_near_loaded_end() {
        let mut document = document_with_lines(50, Some(100));
        let viewport_height = TEXT_PREVIEW_TEXT_SIZE * TEXT_PREVIEW_LINE_HEIGHT * 20.0;

        assert!(document
            .perform(text_editor::Action::Scroll { lines: 19 }, viewport_height)
            .is_none());

        let request = document
            .perform(text_editor::Action::Scroll { lines: 1 }, viewport_height)
            .expect("chunk request");
        assert_eq!(request.start_offset, 100);
        assert_eq!(request.generation, 7);
        assert_eq!(request.line_limit, TEXT_PREVIEW_CHUNK_LINE_LIMIT);

        assert!(document
            .perform(text_editor::Action::Scroll { lines: 1 }, viewport_height)
            .is_none());
    }

    #[test]
    fn text_preview_chunk_append_preserves_existing_content_and_revision() {
        let mut document = document_with_lines(50, Some(100));
        let viewport_height = TEXT_PREVIEW_TEXT_SIZE * TEXT_PREVIEW_LINE_HEIGHT * 20.0;
        let request = document
            .perform(text_editor::Action::Scroll { lines: 20 }, viewport_height)
            .expect("chunk request");
        let scroll_line_offset = document.visual_scroll_line_offset();

        assert!(document.append_chunk(
            TextPreviewChunk {
                start_offset: request.start_offset,
                content: "line 50\nline 51".to_owned(),
                line_count: 2,
                next_offset: None,
                line_limit_notice: None,
            },
            viewport_height,
        ));

        assert!(document.content().text().contains("line 49\nline 50"));
        assert!(document.content().text().contains("line 51"));
        assert_eq!(document.visual_scroll_line_offset(), scroll_line_offset);
        assert_eq!(document.content_revision(), 1);
    }

    #[test]
    fn rendered_markdown_requests_next_chunk_near_content_end() {
        let mut document = document_with_lines(50, Some(100));

        assert!(document
            .request_next_chunk_for_rendered_markdown(100.0, 400.0, 1_200.0)
            .is_none());

        let request = document
            .request_next_chunk_for_rendered_markdown(780.0, 400.0, 1_200.0)
            .expect("chunk request");

        assert_eq!(request.start_offset, 100);
        assert_eq!(request.line_limit, TEXT_PREVIEW_CHUNK_LINE_LIMIT);
        assert!(document
            .request_next_chunk_for_rendered_markdown(800.0, 400.0, 1_200.0)
            .is_none());
    }

    #[test]
    fn text_preview_rejects_stale_chunk_offset() {
        let mut document = document_with_lines(50, Some(100));
        let viewport_height = TEXT_PREVIEW_TEXT_SIZE * TEXT_PREVIEW_LINE_HEIGHT * 20.0;
        document
            .perform(text_editor::Action::Scroll { lines: 20 }, viewport_height)
            .expect("chunk request");

        assert!(!document.append_chunk(
            TextPreviewChunk {
                start_offset: 200,
                content: "stale".to_owned(),
                line_count: 1,
                next_offset: None,
                line_limit_notice: None,
            },
            viewport_height,
        ));
        assert!(!document.content().text().contains("stale"));
    }

    #[test]
    fn text_preview_records_chunk_error_without_dropping_content() {
        let mut document = document_with_lines(50, Some(100));
        let viewport_height = TEXT_PREVIEW_TEXT_SIZE * TEXT_PREVIEW_LINE_HEIGHT * 20.0;
        let request = document
            .perform(text_editor::Action::Scroll { lines: 20 }, viewport_height)
            .expect("chunk request");

        assert!(document.accept_chunk_error(request.start_offset, "could not read".to_owned()));
        assert_eq!(document.chunk_error(), Some("could not read"));
        assert!(document.content().text().contains("line 49"));
    }

    #[test]
    fn loaded_line_count_counts_empty_text_as_zero() {
        assert_eq!(text_preview_loaded_line_count(""), 0);
        assert_eq!(text_preview_loaded_line_count("one"), 1);
        assert_eq!(text_preview_loaded_line_count("one\ntwo"), 2);
    }

    #[test]
    fn markdown_extension_selects_markdown_format() {
        assert_eq!(
            text_preview_format_for_path(Path::new("README.md")),
            TextPreviewFormat::Markdown
        );
        assert_eq!(
            text_preview_format_for_path(Path::new("notes.txt")),
            TextPreviewFormat::Plain
        );
    }

    #[test]
    fn render_markdown_preview_formats_common_blocks() {
        let markdown = "# Title\n\nParagraph with **bold** and [link](https://example.com).\n\n- [x] done\n- next\n\n```rust\nfn main() {}\n```\n";
        let (rendered, format) = render_text_preview(Path::new("README.md"), markdown);

        assert_eq!(format, TextPreviewFormat::Markdown);
        assert_eq!(rendered, markdown);
    }
}
