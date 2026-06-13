use std::path::{Path, PathBuf};

use iced::widget::text_editor;

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

pub(crate) struct TextPreviewDocument {
    path: PathBuf,
    content: text_editor::Content,
    line_numbers: text_editor::Content,
    markdown_preview_mode: MarkdownPreviewMode,
}

impl TextPreviewDocument {
    pub(crate) fn new(path: PathBuf, content: &str, format: TextPreviewFormat) -> Self {
        let content_state = text_editor::Content::with_text(content);
        let line_numbers =
            text_editor::Content::with_text(&line_number_text(content_state.line_count()));

        Self {
            path,
            content: content_state,
            line_numbers,
            markdown_preview_mode: initial_markdown_preview_mode(format),
        }
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        self.path.as_path()
    }

    pub(crate) fn content(&self) -> &text_editor::Content {
        &self.content
    }

    pub(crate) fn line_numbers(&self) -> &text_editor::Content {
        &self.line_numbers
    }

    pub(crate) fn line_number_digit_count(&self) -> usize {
        self.content.line_count().max(1).to_string().len()
    }

    pub(crate) fn markdown_preview_mode(&self) -> MarkdownPreviewMode {
        self.markdown_preview_mode
    }

    pub(crate) fn select_markdown_preview_mode(&mut self, mode: MarkdownPreviewMode) {
        self.markdown_preview_mode = mode;
    }

    pub(crate) fn perform(&mut self, action: text_editor::Action) {
        if let Some(line_number_action) = line_number_sync_action(&action) {
            self.line_numbers.perform(line_number_action);
        }

        if action.is_edit() {
            return;
        }

        self.content.perform(action);
    }
}

fn line_number_sync_action(action: &text_editor::Action) -> Option<text_editor::Action> {
    match action {
        text_editor::Action::Scroll { .. } => Some(action.clone()),
        text_editor::Action::Move(motion) | text_editor::Action::Select(motion) => {
            Some(text_editor::Action::Move(*motion))
        }
        _ => None,
    }
}

fn line_number_text(line_count: usize) -> String {
    let line_count = line_count.max(1);
    let digit_count = line_count.to_string().len();
    (1..=line_count)
        .map(|line_number| format!("{line_number:>digit_count$}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn initial_markdown_preview_mode(format: TextPreviewFormat) -> MarkdownPreviewMode {
    match format {
        TextPreviewFormat::Plain => MarkdownPreviewMode::Raw,
        TextPreviewFormat::Markdown => MarkdownPreviewMode::Rendered,
    }
}

pub(crate) fn render_text_preview(path: &Path, content: &str) -> (String, TextPreviewFormat) {
    let format = text_preview_format_for_path(path);
    (content.to_owned(), format)
}

fn text_preview_format_for_path(path: &Path) -> TextPreviewFormat {
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

    #[test]
    fn text_preview_document_keeps_selectable_text_unmodified() {
        let content = "first line\nsecond line\n";
        let document =
            TextPreviewDocument::new(PathBuf::from("note.txt"), content, TextPreviewFormat::Plain);

        assert_eq!(document.content().text(), content);
        assert_eq!(document.line_numbers().text(), "1\n2\n3");
    }

    #[test]
    fn empty_text_preview_document_keeps_empty_selectable_text() {
        let document =
            TextPreviewDocument::new(PathBuf::from("empty.txt"), "", TextPreviewFormat::Plain);

        assert_eq!(document.content().text(), "");
        assert_eq!(document.line_numbers().text(), "1");
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

    #[test]
    fn line_number_text_uses_independent_right_aligned_gutter_content() {
        assert_eq!(
            line_number_text(12),
            " 1\n 2\n 3\n 4\n 5\n 6\n 7\n 8\n 9\n10\n11\n12"
        );
    }
}
