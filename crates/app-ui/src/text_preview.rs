use std::path::Path;

use crate::model::TextPreviewFormat;

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
    use std::path::Path;

    use super::*;

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
