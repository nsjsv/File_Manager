use iced::widget::{checkbox, column, container, row, scrollable, text_input, Column, Space};
use iced::{Alignment, Element, Length};

use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, context_menu_style,
    path_suggestion_item_style,
};
use crate::formatting::format_middle_ellipsized_text;
use crate::model::{
    BatchRenameCaseRule, BatchRenameMessage, BatchRenamePreviewRow, BatchRenameState, Message,
    ScrollbarRegion, ScrollbarVisibility,
};
use crate::typography::readable_text;

use super::option_controls::{
    inactive_primary_action_button, primary_action_button, secondary_action_button,
    segmented_choice_row, SegmentedChoice,
};

const BATCH_RENAME_PANEL_WIDTH: f32 = 720.0;
const BATCH_RENAME_PREVIEW_HEIGHT: f32 = 240.0;
const BATCH_RENAME_PATH_MAX_CHARS: usize = 38;

pub(super) fn batch_rename_panel(
    state: &BatchRenameState,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    let header = row![
        readable_text("Batch Rename").size(16).width(Length::Fill),
        readable_text(format!("{} items", state.items.len())).size(12),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let content = column![
        header,
        sequence_controls(state),
        replace_controls(state),
        insert_controls(state),
        slice_controls(state),
        case_controls(state),
        preview_rows(state, scrollbar_visibility),
        action_row(state),
    ]
    .spacing(12)
    .width(Length::Fill);

    container(content)
        .padding(14)
        .width(Length::Fixed(BATCH_RENAME_PANEL_WIDTH))
        .style(context_menu_style)
        .into()
}

fn sequence_controls(state: &BatchRenameState) -> Element<'_, Message> {
    column![
        section_title("Sequence"),
        row![
            input_column(
                "Prefix",
                &state.sequence.prefix,
                BatchRenameMessage::SequencePrefixChanged
            ),
            input_column(
                "Start",
                &state.sequence.start_input,
                BatchRenameMessage::SequenceStartChanged
            ),
            input_column(
                "Padding",
                &state.sequence.padding_input,
                BatchRenameMessage::SequencePaddingChanged
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        row![
            checkbox(state.sequence.include_original_stem)
                .label("Original stem")
                .on_toggle(|value| Message::BatchRename(
                    BatchRenameMessage::SequenceIncludeOriginalToggled(value)
                )),
            checkbox(state.sequence.preserve_extension)
                .label("Extension")
                .on_toggle(|value| Message::BatchRename(
                    BatchRenameMessage::SequencePreserveExtensionToggled(value)
                )),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ]
    .spacing(6)
    .into()
}

fn replace_controls(state: &BatchRenameState) -> Element<'_, Message> {
    column![
        section_title("Replace"),
        row![
            input_column(
                "Find",
                &state.replace.find,
                BatchRenameMessage::ReplaceFindChanged
            ),
            input_column(
                "With",
                &state.replace.replacement,
                BatchRenameMessage::ReplaceWithChanged
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(6)
    .into()
}

fn insert_controls(state: &BatchRenameState) -> Element<'_, Message> {
    column![
        section_title("Insert"),
        row![
            input_column(
                "Text",
                &state.insert.text,
                BatchRenameMessage::InsertTextChanged
            ),
            input_column(
                "Position",
                &state.insert.position_input,
                BatchRenameMessage::InsertPositionChanged
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(6)
    .into()
}

fn slice_controls(state: &BatchRenameState) -> Element<'_, Message> {
    column![
        section_title("Slice"),
        row![
            input_column(
                "Start",
                &state.slice.start_input,
                BatchRenameMessage::SliceStartChanged
            ),
            input_column(
                "Length",
                &state.slice.length_input,
                BatchRenameMessage::SliceLengthChanged
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(6)
    .into()
}

fn case_controls(state: &BatchRenameState) -> Element<'static, Message> {
    column![
        section_title("Case"),
        segmented_choice_row(vec![
            case_choice(state, BatchRenameCaseRule::Unchanged),
            case_choice(state, BatchRenameCaseRule::Lowercase),
            case_choice(state, BatchRenameCaseRule::Uppercase),
            case_choice(state, BatchRenameCaseRule::TitleCase),
        ]),
    ]
    .spacing(6)
    .into()
}

fn case_choice(state: &BatchRenameState, case: BatchRenameCaseRule) -> SegmentedChoice {
    SegmentedChoice {
        label: case.label(),
        selected: state.case == case,
        message: Message::BatchRename(BatchRenameMessage::CaseSelected(case)),
    }
}

fn preview_rows(
    state: &BatchRenameState,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    let mut rows = Column::new().spacing(4);
    for row in &state.preview.rows {
        rows = rows.push(preview_row(row));
    }

    let scroll_region = ScrollbarRegion::BatchRenamePreview;
    column![
        section_title("Preview"),
        scrollable(smooth_scroll_content(rows, scroll_region.clone()))
            .id(smooth_scroll_id(&scroll_region))
            .direction(auto_hide_vertical_scrollbar_direction(
                scrollbar_visibility,
                6.0,
            ))
            .style(auto_hide_scrollbar_style(scrollbar_visibility))
            .height(Length::Fixed(BATCH_RENAME_PREVIEW_HEIGHT))
            .on_scroll(|_| Message::BatchRenamePreviewScrolled),
    ]
    .spacing(6)
    .into()
}

fn preview_row(row_state: &BatchRenamePreviewRow) -> Element<'_, Message> {
    let source =
        format_middle_ellipsized_text(&row_state.original_name, BATCH_RENAME_PATH_MAX_CHARS);
    let target = format_middle_ellipsized_text(&row_state.target_name, BATCH_RENAME_PATH_MAX_CHARS);
    let status = row_state.status.label();
    container(
        row![
            readable_text(source).size(12).width(Length::FillPortion(3)),
            readable_text(target).size(12).width(Length::FillPortion(3)),
            readable_text(status).size(11).width(Length::FillPortion(1)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([6, 8])
    .width(Length::Fill)
    .style(path_suggestion_item_style)
    .into()
}

fn action_row(state: &BatchRenameState) -> Element<'static, Message> {
    let apply = if state.can_apply() {
        primary_action_button("Apply", Message::BatchRename(BatchRenameMessage::Apply))
    } else {
        inactive_primary_action_button("Apply")
    };

    row![
        Space::new().width(Length::Fill),
        secondary_action_button("Cancel", Message::BatchRename(BatchRenameMessage::Cancel)),
        apply,
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn input_column<'a>(
    label: &'static str,
    value: &'a str,
    message: fn(String) -> BatchRenameMessage,
) -> Element<'a, Message> {
    column![
        readable_text(label).size(11),
        text_input(label, value)
            .on_input(move |value| Message::BatchRename(message(value)))
            .padding([6, 8])
            .size(13)
            .width(Length::Fill),
    ]
    .spacing(3)
    .width(Length::Fill)
    .into()
}

fn section_title(label: &'static str) -> Element<'static, Message> {
    readable_text(label).size(12).into()
}
