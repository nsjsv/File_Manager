use iced::mouse;
use iced::widget::{button, column, mouse_area, row, text_editor, Button, Column};
use iced::{Alignment, Element, Font, Length};

use crate::appearance::{
    navigation_icon_button_style, text_preview_editor_style, text_preview_line_number_style,
};
use crate::model::{
    MarkdownPreviewMode, Message, ScrollbarVisibility, TextPreviewDocument, TextPreviewFormat,
};
use crate::typography::readable_text;

use super::markdown_preview::markdown_preview_body;

const MARKDOWN_MODE_SWITCH_RESERVED_HEIGHT: f32 = 40.0;
const MARKDOWN_MIN_BODY_SCROLL_HEIGHT: f32 = 120.0;
const TEXT_PREVIEW_EDITOR_PADDING: u16 = 8;
const TEXT_PREVIEW_GUTTER_HORIZONTAL_PADDING: u16 = 6;
const TEXT_PREVIEW_GUTTER_DIGIT_WIDTH: f32 = 10.0;
const TEXT_PREVIEW_GUTTER_MIN_WIDTH: f32 = 30.0;
const TEXT_PREVIEW_GUTTER_SPACING: f32 = 4.0;

pub(super) fn text_preview_panel<'a>(
    rendered: &'a str,
    format: TextPreviewFormat,
    document: Option<&'a TextPreviewDocument>,
    scroll_height: f32,
    scrollbar_visibility: ScrollbarVisibility,
) -> Column<'a, Message> {
    let body: Element<'_, Message> = match format {
        TextPreviewFormat::Plain => plain_text_preview_body(document, scroll_height),
        TextPreviewFormat::Markdown => {
            markdown_text_preview_body(rendered, document, scroll_height, scrollbar_visibility)
        }
    };

    column![body]
}

fn plain_text_preview_body<'a>(
    document: Option<&'a TextPreviewDocument>,
    scroll_height: f32,
) -> Element<'a, Message> {
    if let Some(document) = document {
        row![
            text_preview_line_number_gutter(document, scroll_height),
            text_editor(document.content())
                .placeholder("(empty file)")
                .height(Length::Fixed(scroll_height))
                .font(Font::MONOSPACE)
                .padding(TEXT_PREVIEW_EDITOR_PADDING)
                .style(text_preview_editor_style())
                .on_action(Message::TextPreviewAction),
        ]
        .spacing(TEXT_PREVIEW_GUTTER_SPACING)
        .into()
    } else {
        readable_text("Text preview is not ready").size(14).into()
    }
}

fn text_preview_line_number_gutter<'a>(
    document: &'a TextPreviewDocument,
    scroll_height: f32,
) -> Element<'a, Message> {
    let gutter = text_editor(document.line_numbers())
        .height(Length::Fixed(scroll_height))
        .width(text_preview_line_number_gutter_width(document))
        .font(Font::MONOSPACE)
        .padding([
            TEXT_PREVIEW_EDITOR_PADDING,
            TEXT_PREVIEW_GUTTER_HORIZONTAL_PADDING,
        ])
        .style(text_preview_line_number_style());

    mouse_area(gutter)
        .on_scroll(|delta| Message::TextPreviewAction(text_preview_scroll_action(delta)))
        .into()
}

fn text_preview_line_number_gutter_width(document: &TextPreviewDocument) -> f32 {
    let digit_width = document.line_number_digit_count() as f32 * TEXT_PREVIEW_GUTTER_DIGIT_WIDTH;
    let padding_width = f32::from(TEXT_PREVIEW_GUTTER_HORIZONTAL_PADDING) * 2.0;

    (digit_width + padding_width).max(TEXT_PREVIEW_GUTTER_MIN_WIDTH)
}

fn text_preview_scroll_action(delta: mouse::ScrollDelta) -> text_editor::Action {
    let lines = match delta {
        mouse::ScrollDelta::Lines { y, .. } => {
            if y.abs() > 0.0 {
                y.signum() * -(y.abs() * 4.0).max(1.0)
            } else {
                0.0
            }
        }
        mouse::ScrollDelta::Pixels { y, .. } => -y / 4.0,
    };

    text_editor::Action::Scroll {
        lines: lines as i32,
    }
}

fn markdown_text_preview_body<'a>(
    rendered: &'a str,
    document: Option<&'a TextPreviewDocument>,
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
        MarkdownPreviewMode::Rendered => {
            markdown_preview_body(rendered, body_height, scrollbar_visibility)
        }
        MarkdownPreviewMode::Raw => plain_text_preview_body(Some(document), body_height),
    };

    column![markdown_preview_mode_switch(mode), body]
        .spacing(8)
        .into()
}

fn markdown_preview_mode_switch(mode: MarkdownPreviewMode) -> Element<'static, Message> {
    row![
        markdown_preview_mode_button("Rendered", MarkdownPreviewMode::Rendered, mode),
        markdown_preview_mode_button("Raw", MarkdownPreviewMode::Raw, mode),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn markdown_preview_mode_button(
    label: &'static str,
    mode: MarkdownPreviewMode,
    selected_mode: MarkdownPreviewMode,
) -> Button<'static, Message> {
    let label = if mode == selected_mode {
        format!("[{label}]")
    } else {
        label.to_owned()
    };
    let button = button(readable_text(label).size(12))
        .padding([4, 8])
        .style(navigation_icon_button_style());
    if mode == selected_mode {
        button
    } else {
        button.on_press(Message::MarkdownPreviewModeSelected(mode))
    }
}
