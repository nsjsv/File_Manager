use iced::widget::{column, text_editor, Column};
use iced::Element;

use crate::model::{
    MarkdownPreviewMode, Message, ScrollbarVisibility, TextPreviewDocument, TextPreviewFormat,
    TextPreviewLineLimitNotice,
};
use crate::text_preview_viewer::text_preview_viewer;
use crate::typography::{localized_text, readable_text};

use super::markdown_preview::markdown_preview_body;
use super::option_controls::{segmented_choice_row, SegmentedChoice};

const MARKDOWN_MODE_SWITCH_RESERVED_HEIGHT: f32 = 40.0;
const MARKDOWN_MIN_BODY_SCROLL_HEIGHT: f32 = 120.0;
const TEXT_PREVIEW_LIMIT_NOTICE_RESERVED_HEIGHT: f32 = 30.0;
const TEXT_PREVIEW_MIN_BODY_SCROLL_HEIGHT: f32 = 120.0;

pub(super) fn text_preview_panel<'a>(
    rendered: &'a str,
    format: TextPreviewFormat,
    line_limit_notice: Option<TextPreviewLineLimitNotice>,
    document: Option<&'a TextPreviewDocument>,
    scroll_height: f32,
    scrollbar_visibility: ScrollbarVisibility,
) -> Column<'a, Message> {
    let line_limit_notice = document
        .and_then(TextPreviewDocument::line_limit_notice)
        .or(line_limit_notice);
    let chunk_error = document.and_then(TextPreviewDocument::chunk_error);
    let external_notice_is_visible =
        text_preview_external_notice_is_visible(format, document, line_limit_notice);
    let footer_count = usize::from(external_notice_is_visible) + usize::from(chunk_error.is_some());
    let body_height = if footer_count > 0 {
        (scroll_height - TEXT_PREVIEW_LIMIT_NOTICE_RESERVED_HEIGHT * footer_count as f32)
            .max(TEXT_PREVIEW_MIN_BODY_SCROLL_HEIGHT)
    } else {
        scroll_height
    };
    let body: Element<'_, Message> = match format {
        TextPreviewFormat::Plain => plain_text_preview_body(document, body_height),
        TextPreviewFormat::Markdown => markdown_text_preview_body(
            rendered,
            document,
            line_limit_notice,
            body_height,
            scrollbar_visibility,
        ),
    };

    let mut panel = column![body].spacing(8);
    if external_notice_is_visible {
        let Some(notice) = line_limit_notice else {
            return panel;
        };
        panel = panel.push(text_preview_line_limit_notice(notice));
    }
    if let Some(error) = chunk_error {
        panel = panel.push(text_preview_chunk_error(error));
    }
    panel
}

fn plain_text_preview_body<'a>(
    document: Option<&'a TextPreviewDocument>,
    scroll_height: f32,
) -> Element<'a, Message> {
    if let Some(document) = document {
        text_preview_viewer(document, scroll_height, move |lines| {
            Message::TextPreviewAction {
                action: text_editor::Action::Scroll { lines },
                viewport_height: scroll_height,
            }
        })
    } else {
        readable_text("Text preview is not ready").size(14).into()
    }
}

fn text_preview_line_limit_notice(notice: TextPreviewLineLimitNotice) -> Element<'static, Message> {
    readable_text(notice.label()).size(12).into()
}

fn text_preview_chunk_error(error: &str) -> Element<'static, Message> {
    localized_text(format!("Could not load more text preview: {error}"))
        .size(12)
        .into()
}

fn text_preview_external_notice_is_visible(
    format: TextPreviewFormat,
    document: Option<&TextPreviewDocument>,
    notice: Option<TextPreviewLineLimitNotice>,
) -> bool {
    notice.is_some_and(|_| {
        document
            .filter(|document| {
                format == TextPreviewFormat::Plain
                    || document.markdown_preview_mode() == MarkdownPreviewMode::Raw
            })
            .map(TextPreviewDocument::is_scrolled_to_preview_end)
            .unwrap_or(false)
    })
}

fn markdown_text_preview_body<'a>(
    rendered: &'a str,
    document: Option<&'a TextPreviewDocument>,
    line_limit_notice: Option<TextPreviewLineLimitNotice>,
    scroll_height: f32,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'a, Message> {
    let Some(document) = document else {
        return readable_text("Text preview is not ready").size(14).into();
    };
    let mode = document.markdown_preview_mode();
    let body_height =
        (scroll_height - MARKDOWN_MODE_SWITCH_RESERVED_HEIGHT).max(MARKDOWN_MIN_BODY_SCROLL_HEIGHT);
    let body = match mode {
        MarkdownPreviewMode::Rendered => markdown_preview_body(
            rendered,
            line_limit_notice,
            body_height,
            scrollbar_visibility,
        ),
        MarkdownPreviewMode::Raw => plain_text_preview_body(Some(document), body_height),
    };

    column![markdown_preview_mode_switch(mode), body]
        .spacing(8)
        .into()
}

fn markdown_preview_mode_switch(mode: MarkdownPreviewMode) -> Element<'static, Message> {
    segmented_choice_row(vec![
        markdown_preview_mode_choice("Rendered", MarkdownPreviewMode::Rendered, mode),
        markdown_preview_mode_choice("Raw", MarkdownPreviewMode::Raw, mode),
    ])
}

fn markdown_preview_mode_choice(
    label: &'static str,
    mode: MarkdownPreviewMode,
    selected_mode: MarkdownPreviewMode,
) -> SegmentedChoice {
    SegmentedChoice {
        label,
        selected: mode == selected_mode,
        message: Message::MarkdownPreviewModeSelected(mode),
    }
}
