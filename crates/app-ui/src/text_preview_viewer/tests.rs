use iced::advanced::text;
use iced::advanced::text::editor::Editor as _;
use iced::advanced::text::highlighter::PlainText;
use iced::widget::text_editor;
use iced::{Font, Padding, Pixels, Size};

use super::*;

fn preview_editor_for_text(content: &str, width: f32, height: f32) -> PreviewEditor {
    let mut editor = PreviewEditor::with_text(content);
    let padding = Padding::new(TEXT_PREVIEW_VIEWER_PADDING);
    editor.update(
        Size::new(
            (width - padding.x()).max(0.0),
            (height - padding.y()).max(0.0),
        ),
        Font::MONOSPACE,
        Pixels(TEXT_PREVIEW_TEXT_SIZE),
        text::LineHeight::Relative(TEXT_PREVIEW_LINE_HEIGHT),
        text::Wrapping::WordOrGlyph,
        &mut PlainText,
    );
    editor
}

#[test]
fn line_number_offsets_only_returns_visible_rows() {
    let content = (0..1_000)
        .map(|line_number| format!("line {line_number}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut editor = preview_editor_for_text(&content, 400.0, 120.0);
    editor.perform(text_editor::Action::Scroll { lines: 500 });

    let offsets = line_number_offsets(&editor, 120.0);

    assert!(offsets.len() < 12);
    assert!(offsets.iter().all(|(line_index, _)| *line_index < 510));
    assert!(offsets.iter().any(|(line_index, _)| *line_index >= 500));
}

#[test]
fn visible_logical_lines_keep_late_file_rows_viewport_local() {
    let content = (0..6_000)
        .map(|line_number| format!("line {line_number}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut editor = preview_editor_for_text(&content, 400.0, 120.0);
    editor.perform(text_editor::Action::Scroll { lines: 4_000 });

    let visible_lines = visible_logical_lines(&editor, 120.0);

    assert!(visible_lines.iter().any(|line| line.line_index > 3_374));
    assert!(visible_lines
        .iter()
        .all(|line| line.y > -100.0 && line.y < 220.0));
}

#[test]
fn bounded_scroll_rejects_overscroll_at_file_edges() {
    let content = (0..100)
        .map(|line_number| format!("line {line_number}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut state = TextPreviewViewerState {
        editor: preview_editor_for_text(&content, 400.0, 120.0),
        ..Default::default()
    };

    assert!(!apply_bounded_scroll_lines(&mut state, -10, 120.0));
    assert_eq!(editor_scroll_y(&state.editor), 0.0);

    assert!(apply_bounded_scroll_lines(&mut state, 10_000, 120.0));
    let bottom_scroll_y = editor_scroll_y(&state.editor);
    assert!(bottom_scroll_y <= max_editor_scroll_y(&state.editor, 120.0));
    assert!(!apply_bounded_scroll_lines(&mut state, 10_000, 120.0));
    assert_eq!(editor_scroll_y(&state.editor), bottom_scroll_y);
}

#[test]
fn line_number_offsets_use_logical_lines_for_wrapped_text() {
    let content =
        "short\nthis is a very long line that should wrap several times in a narrow preview";
    let editor = preview_editor_for_text(content, 80.0, 240.0);

    let offsets = line_number_offsets(&editor, 240.0);

    assert_eq!(offsets.len(), 2);
    assert_eq!(offsets[0].0, 0);
    assert_eq!(offsets[1].0, 1);
    assert!(offsets[1].1 > offsets[0].1);
}
