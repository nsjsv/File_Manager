use iced::widget::{
    button, checkbox, column, container, mouse_area, pick_list, row, scrollable, text_input,
    Column, Space,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Theme};

use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, base_text_color,
    context_menu_style, dragged_row_style, is_dark_theme, muted_text_color,
    path_suggestion_item_style, subtle_border_color,
};
use crate::formatting::format_middle_ellipsized_text;
use crate::model::{
    BatchRenameCaseRule, BatchRenameExtensionMode, BatchRenameMessage, BatchRenamePreviewRow,
    BatchRenameRandomMode, BatchRenameRulePanel, BatchRenameSortMode, BatchRenameState, Message,
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
        rule_panel_selector(state),
        active_rule_controls(state),
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

fn rule_panel_selector(state: &BatchRenameState) -> Element<'static, Message> {
    let mut tabs = row![].spacing(4).align_y(Alignment::Center);
    for panel in BatchRenameRulePanel::options() {
        tabs = tabs.push(rule_panel_tab(panel, state.active_panel == panel));
    }

    container(tabs).width(Length::Fill).into()
}

fn rule_panel_tab(panel: BatchRenameRulePanel, selected: bool) -> Element<'static, Message> {
    let tab = button(readable_text(rule_panel_label(panel)).size(11))
        .padding([5, 8])
        .height(Length::Fixed(28.0))
        .style(rule_panel_tab_style(selected));

    if selected {
        tab.into()
    } else {
        tab.on_press(Message::BatchRename(BatchRenameMessage::RulePanelSelected(
            panel,
        )))
        .into()
    }
}

fn rule_panel_label(panel: BatchRenameRulePanel) -> &'static str {
    match panel {
        BatchRenameRulePanel::Sort => "Sort",
        BatchRenameRulePanel::Extension => "Ext",
        BatchRenameRulePanel::Case => "Case",
        BatchRenameRulePanel::Sequence => "Seq",
        BatchRenameRulePanel::Replace => "Replace",
        BatchRenameRulePanel::Insert => "Insert",
        BatchRenameRulePanel::Slice => "Slice",
        BatchRenameRulePanel::Random => "Random",
        BatchRenameRulePanel::Remove => "Remove",
        BatchRenameRulePanel::List => "List",
        BatchRenameRulePanel::Custom => "Custom",
        BatchRenameRulePanel::Regex => "Regex",
        BatchRenameRulePanel::Batch => "Batch",
    }
}

fn rule_panel_tab_style(
    selected: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + Clone {
    move |theme, status| {
        let accent = if is_dark_theme(theme) {
            Color::from_rgb8(125, 179, 255)
        } else {
            Color::from_rgb8(74, 137, 220)
        };
        let hover = if is_dark_theme(theme) {
            Color::from_rgb8(32, 43, 59)
        } else {
            Color::from_rgb8(239, 245, 255)
        };

        button::Style {
            background: Some(Background::Color(if selected {
                Color { a: 0.18, ..accent }
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                hover
            } else {
                Color::TRANSPARENT
            })),
            text_color: if selected {
                accent
            } else if matches!(status, button::Status::Disabled) {
                muted_text_color(theme)
            } else {
                base_text_color(theme)
            },
            border: Border {
                color: if selected {
                    accent
                } else {
                    subtle_border_color(theme)
                },
                width: if selected { 1.0 } else { 0.0 },
                radius: 6.0.into(),
            },
            ..button::Style::default()
        }
    }
}

fn active_rule_controls(state: &BatchRenameState) -> Element<'_, Message> {
    match state.active_panel {
        BatchRenameRulePanel::Sort => sort_controls(state),
        BatchRenameRulePanel::Extension => extension_controls(state),
        BatchRenameRulePanel::Case => case_controls(state),
        BatchRenameRulePanel::Sequence => sequence_controls(state),
        BatchRenameRulePanel::Replace => replace_controls(state),
        BatchRenameRulePanel::Insert => insert_controls(state),
        BatchRenameRulePanel::Slice => slice_controls(state),
        BatchRenameRulePanel::Random => random_controls(state),
        BatchRenameRulePanel::Remove => remove_controls(state),
        BatchRenameRulePanel::List => list_controls(state),
        BatchRenameRulePanel::Custom => custom_controls(state),
        BatchRenameRulePanel::Regex => regex_controls(state),
        BatchRenameRulePanel::Batch => batch_controls(state),
    }
}

fn sort_controls(state: &BatchRenameState) -> Element<'_, Message> {
    column![
        section_title("Sort"),
        pick_list(
            BatchRenameSortMode::options(),
            Some(state.sort.mode),
            |mode| Message::BatchRename(BatchRenameMessage::SortModeSelected(mode)),
        )
        .width(Length::Fill)
        .text_size(12)
        .padding([5, 8]),
    ]
    .spacing(6)
    .into()
}

fn extension_controls(state: &BatchRenameState) -> Element<'_, Message> {
    column![
        section_title("Extension"),
        row![
            pick_list(
                BatchRenameExtensionMode::options(),
                Some(state.extension.mode),
                |mode| Message::BatchRename(BatchRenameMessage::ExtensionModeSelected(mode)),
            )
            .width(Length::Fill)
            .text_size(12)
            .padding([5, 8]),
            input_column(
                "New extension",
                &state.extension.replacement,
                BatchRenameMessage::ExtensionReplacementChanged
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(6)
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

fn random_controls(state: &BatchRenameState) -> Element<'_, Message> {
    column![
        section_title("Random"),
        row![
            pick_list(
                BatchRenameRandomMode::options(),
                Some(state.random.mode),
                |mode| Message::BatchRename(BatchRenameMessage::RandomModeSelected(mode)),
            )
            .width(Length::Fill)
            .text_size(12)
            .padding([5, 8]),
            input_column(
                "Length",
                &state.random.length_input,
                BatchRenameMessage::RandomLengthChanged
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        input_column(
            "Alphabet",
            &state.random.alphabet,
            BatchRenameMessage::RandomAlphabetChanged
        ),
    ]
    .spacing(6)
    .into()
}

fn remove_controls(state: &BatchRenameState) -> Element<'_, Message> {
    column![
        section_title("Remove"),
        input_column(
            "Text",
            &state.remove.text,
            BatchRenameMessage::RemoveTextChanged
        ),
        row![
            input_column(
                "Start",
                &state.remove.start_input,
                BatchRenameMessage::RemoveStartChanged
            ),
            input_column(
                "Length",
                &state.remove.length_input,
                BatchRenameMessage::RemoveLengthChanged
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(6)
    .into()
}

fn list_controls(state: &BatchRenameState) -> Element<'_, Message> {
    column![
        section_title("List"),
        input_column(
            "Names",
            &state.list.names,
            BatchRenameMessage::ListNamesChanged
        ),
    ]
    .spacing(6)
    .into()
}

fn custom_controls(state: &BatchRenameState) -> Element<'_, Message> {
    column![
        section_title("Custom"),
        input_column(
            "Template",
            &state.custom.template,
            BatchRenameMessage::CustomTemplateChanged
        ),
    ]
    .spacing(6)
    .into()
}

fn regex_controls(state: &BatchRenameState) -> Element<'_, Message> {
    column![
        section_title("Regex"),
        row![
            input_column(
                "Pattern",
                &state.regex.pattern,
                BatchRenameMessage::RegexPatternChanged
            ),
            input_column(
                "Replacement",
                &state.regex.replacement,
                BatchRenameMessage::RegexReplacementChanged
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(6)
    .into()
}

fn batch_controls(state: &BatchRenameState) -> Element<'_, Message> {
    column![
        section_title("Batch"),
        input_column(
            "Commands",
            &state.batch.commands,
            BatchRenameMessage::BatchCommandsChanged
        ),
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
    let dragging_active = state.dragging_preview_source().is_some();
    for row in &state.preview.rows {
        let is_dragging = state.dragging_preview_source() == Some(row.source.as_path());
        rows = rows.push(preview_row(row, is_dragging, dragging_active));
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

fn preview_row(
    row_state: &BatchRenamePreviewRow,
    is_dragging: bool,
    dragging_active: bool,
) -> Element<'_, Message> {
    let source =
        format_middle_ellipsized_text(&row_state.original_name, BATCH_RENAME_PATH_MAX_CHARS);
    let target = format_middle_ellipsized_text(&row_state.target_name, BATCH_RENAME_PATH_MAX_CHARS);
    let status = row_state.status.label();
    let row_container = container(
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
    .style(if is_dragging {
        dragged_row_style
    } else {
        path_suggestion_item_style
    });

    let row_area = mouse_area(row_container)
        .on_press(Message::BatchRename(
            BatchRenameMessage::PreviewDragStarted(row_state.source.clone()),
        ))
        .on_release(Message::BatchRename(
            BatchRenameMessage::PreviewDragFinished,
        ))
        .interaction(iced::mouse::Interaction::Pointer);

    let row_area = if dragging_active {
        row_area.on_enter(Message::BatchRename(
            BatchRenameMessage::PreviewDragEntered(row_state.source.clone()),
        ))
    } else {
        row_area
    };

    row_area.into()
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
