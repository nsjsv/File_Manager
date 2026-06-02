use std::path::Path;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::model::TextPreviewFormat;

pub(crate) fn render_text_preview(path: &Path, content: &str) -> (String, TextPreviewFormat) {
    let format = text_preview_format_for_path(path);
    let rendered = match format {
        TextPreviewFormat::Plain => content.to_owned(),
        TextPreviewFormat::Markdown => render_markdown_preview(content),
    };

    (rendered, format)
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

fn render_markdown_preview(markdown: &str) -> String {
    if markdown.trim().is_empty() {
        return String::new();
    }

    let mut renderer = MarkdownTextRenderer::default();
    for event in Parser::new_ext(markdown, markdown_options()) {
        renderer.push_event(event);
    }

    let rendered = renderer.finish();
    if rendered.trim().is_empty() {
        markdown.to_owned()
    } else {
        rendered
    }
}

fn markdown_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    options
}

#[derive(Default)]
struct MarkdownTextRenderer {
    output: String,
    list_stack: Vec<MarkdownListState>,
    quote_depth: usize,
    link_destinations: Vec<String>,
    image_destinations: Vec<String>,
    heading: Option<MarkdownHeading>,
    in_code_block: bool,
}

impl MarkdownTextRenderer {
    fn push_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.output.push_str(&text),
            Event::Code(code) | Event::InlineMath(code) | Event::DisplayMath(code) => {
                self.output.push_str(&code)
            }
            Event::SoftBreak | Event::HardBreak => self.push_line_break(),
            Event::Rule => {
                self.ensure_blank_line();
                self.output.push_str("---");
                self.finish_block();
            }
            Event::TaskListMarker(checked) => {
                self.output.push_str(if checked { "[x] " } else { "[ ] " });
            }
            Event::FootnoteReference(label) => {
                self.output.push_str("[footnote: ");
                self.output.push_str(&label);
                self.output.push(']');
            }
            Event::Html(_) | Event::InlineHtml(_) => {}
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.ensure_blank_line(),
            Tag::Heading { level, .. } => self.start_heading(level),
            Tag::BlockQuote(_) => {
                self.ensure_blank_line();
                self.quote_depth = self.quote_depth.saturating_add(1);
            }
            Tag::CodeBlock(kind) => self.start_code_block(kind),
            Tag::List(start) => {
                self.ensure_blank_line();
                self.list_stack
                    .push(MarkdownListState { next_number: start });
            }
            Tag::Item => self.start_list_item(),
            Tag::Link { dest_url, .. } => self.link_destinations.push(dest_url.to_string()),
            Tag::Image { dest_url, .. } => {
                self.image_destinations.push(dest_url.to_string());
                self.output.push_str("Image: ");
            }
            Tag::Table(_) => self.ensure_blank_line(),
            Tag::TableRow => self.ensure_line_start(),
            Tag::HtmlBlock
            | Tag::TableHead
            | Tag::TableCell
            | Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.finish_block(),
            TagEnd::Heading(level) => self.end_heading(level),
            TagEnd::BlockQuote(_) => {
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.finish_block();
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.finish_block();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                self.finish_block();
            }
            TagEnd::Item => self.finish_block(),
            TagEnd::Link => self.finish_link(),
            TagEnd::Image => self.finish_image(),
            TagEnd::Table | TagEnd::TableHead => self.finish_block(),
            TagEnd::TableRow => {
                self.trim_trailing_cell_separator();
                self.finish_block();
            }
            TagEnd::TableCell => self.output.push_str(" | "),
            TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::MetadataBlock(_) => {}
        }
    }

    fn start_heading(&mut self, level: HeadingLevel) {
        self.ensure_blank_line();
        if !heading_uses_underline(level) {
            self.output
                .push_str(&"#".repeat(heading_level_number(level)));
            self.output.push(' ');
        }
        self.heading = Some(MarkdownHeading {
            level,
            content_start: self.output.len(),
        });
    }

    fn end_heading(&mut self, fallback_level: HeadingLevel) {
        let heading = self.heading.take().unwrap_or(MarkdownHeading {
            level: fallback_level,
            content_start: self.output.len(),
        });
        if heading_uses_underline(heading.level) {
            let underline = if heading.level == HeadingLevel::H1 {
                '='
            } else {
                '-'
            };
            let width = self.output[heading.content_start..]
                .trim()
                .chars()
                .count()
                .max(1);
            self.push_line_break();
            self.output.push_str(&underline.to_string().repeat(width));
        }
        self.finish_block();
    }

    fn start_code_block(&mut self, kind: CodeBlockKind<'_>) {
        self.ensure_blank_line();
        if let CodeBlockKind::Fenced(language) = kind {
            let language = language.trim();
            if !language.is_empty() {
                self.output.push_str("[code: ");
                self.output.push_str(language);
                self.output.push_str("]\n");
            }
        }
        self.in_code_block = true;
    }

    fn start_list_item(&mut self) {
        self.ensure_line_start();
        self.push_quote_prefix();
        for _ in 0..self.list_stack.len().saturating_sub(1) {
            self.output.push_str("  ");
        }
        let marker = self.next_list_marker();
        self.output.push_str(&marker);
    }

    fn next_list_marker(&mut self) -> String {
        let Some(state) = self.list_stack.last_mut() else {
            return "- ".to_owned();
        };

        match state.next_number {
            Some(number) => {
                state.next_number = Some(number.saturating_add(1));
                format!("{number}. ")
            }
            None => "- ".to_owned(),
        }
    }

    fn finish_link(&mut self) {
        let Some(destination) = self.link_destinations.pop() else {
            return;
        };
        if !destination.is_empty() {
            self.output.push_str(" (");
            self.output.push_str(&destination);
            self.output.push(')');
        }
    }

    fn finish_image(&mut self) {
        let Some(destination) = self.image_destinations.pop() else {
            return;
        };
        if !destination.is_empty() {
            self.output.push_str(" (");
            self.output.push_str(&destination);
            self.output.push(')');
        }
    }

    fn ensure_blank_line(&mut self) {
        self.trim_trailing_spaces();
        if self.output.trim().is_empty() {
            self.output.clear();
        } else if !self.output.ends_with("\n\n") {
            if self.output.ends_with('\n') {
                self.output.push('\n');
            } else {
                self.output.push_str("\n\n");
            }
        }
        self.push_quote_prefix();
    }

    fn ensure_line_start(&mut self) {
        self.trim_trailing_spaces();
        if !self.output.is_empty() && !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    fn push_line_break(&mut self) {
        self.trim_trailing_spaces();
        self.output.push('\n');
        if !self.in_code_block {
            self.push_quote_prefix();
        }
    }

    fn finish_block(&mut self) {
        self.trim_trailing_spaces();
        if !self.output.is_empty() && !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    fn push_quote_prefix(&mut self) {
        for _ in 0..self.quote_depth {
            self.output.push_str("> ");
        }
    }

    fn trim_trailing_spaces(&mut self) {
        while self.output.ends_with(' ') || self.output.ends_with('\t') {
            self.output.pop();
        }
    }

    fn trim_trailing_cell_separator(&mut self) {
        if self.output.ends_with(" | ") {
            let new_len = self.output.len().saturating_sub(3);
            self.output.truncate(new_len);
        }
    }

    fn finish(mut self) -> String {
        self.trim_trailing_spaces();
        self.output.trim_end().to_owned()
    }
}

struct MarkdownListState {
    next_number: Option<u64>,
}

#[derive(Clone, Copy)]
struct MarkdownHeading {
    level: HeadingLevel,
    content_start: usize,
}

fn heading_uses_underline(level: HeadingLevel) -> bool {
    matches!(level, HeadingLevel::H1 | HeadingLevel::H2)
}

fn heading_level_number(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
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
        let rendered = render_markdown_preview(
            "# Title\n\nParagraph with **bold** and [link](https://example.com).\n\n- [x] done\n- next\n\n```rust\nfn main() {}\n```\n",
        );

        assert!(rendered.contains("Title\n====="));
        assert!(rendered.contains("Paragraph with bold and link (https://example.com)."));
        assert!(rendered.contains("- [x] done"));
        assert!(rendered.contains("[code: rust]\nfn main() {}"));
        assert!(!rendered.contains("**bold**"));
    }
}
