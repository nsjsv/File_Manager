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
    markdown_preview_mode: MarkdownPreviewMode,
}

impl TextPreviewDocument {
    pub(crate) fn new(path: PathBuf, content: &str, format: TextPreviewFormat) -> Self {
        Self {
            path,
            content: text_editor::Content::with_text(content),
            markdown_preview_mode: initial_markdown_preview_mode(format),
        }
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        self.path.as_path()
    }

    pub(crate) fn content(&self) -> &text_editor::Content {
        &self.content
    }

    pub(crate) fn markdown_preview_mode(&self) -> MarkdownPreviewMode {
        self.markdown_preview_mode
    }

    pub(crate) fn select_markdown_preview_mode(&mut self, mode: MarkdownPreviewMode) {
        self.markdown_preview_mode = mode;
    }

    pub(crate) fn perform(&mut self, action: text_editor::Action) {
        if action.is_edit() {
            return;
        }

        self.content.perform(action);
    }
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
    }

    #[test]
    fn empty_text_preview_document_keeps_empty_selectable_text() {
        let document =
            TextPreviewDocument::new(PathBuf::from("empty.txt"), "", TextPreviewFormat::Plain);

        assert_eq!(document.content().text(), "");
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
