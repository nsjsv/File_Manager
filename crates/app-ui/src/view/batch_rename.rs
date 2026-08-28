use crate::app::scrollbar::{enhanced_scrollbar, scrollbar_on_scroll, ScrollbarAxis};
use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use iced::widget::{
    button, checkbox, column, container, mouse_area, pick_list, row, scrollable, text_input,
    Column, Space,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Theme};

use crate::anchored_popup::anchored_popup;
use crate::appearance::{
    base_text_color, context_menu_item_button_style, context_menu_style, dragged_row_style,
    enhanced_scrollbar_style, enhanced_vertical_scrollbar_direction, muted_text_color,
    path_suggestion_item_style, subtle_border_color,
};
use crate::formatting::format_middle_ellipsized_text;
use crate::matugen_theme::ui_colors;
use crate::model::{
    BatchRenameCaseRule, BatchRenameExtensionMode, BatchRenameInsertMode, BatchRenameMessage,
    BatchRenamePreviewRow, BatchRenameRandomMode, BatchRenameRemoveClass, BatchRenameRemoveMode,
    BatchRenameReplaceScope, BatchRenameRulePanel, BatchRenameSimpleKind, BatchRenameSimpleToken,
    BatchRenameSliceMode, BatchRenameSortMode, BatchRenameState, Message, ScrollbarRegion,
    ScrollbarViewport, ScrollbarVisibility,
};
use crate::typography::{localized_text, readable_text};

use super::batch_rename_preview_name_input_id;
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
    scrollbar_viewport: Option<ScrollbarViewport>,
) -> Element<'_, Message> {
    let header = row![
        readable_text("Batch Rename").size(16).width(Length::Fill),
        localized_text(format!("{} items", state.items.len())).size(12),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let content = column![
        header,
        mode_selector(state),
        if state.simple_mode {
            simple_controls(state)
        } else {
            column![rule_panel_selector(state), active_rule_controls(state)]
                .spacing(12)
                .width(Length::Fill)
                .into()
        },
        preview_rows(state, scrollbar_visibility, scrollbar_viewport),
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
        let colors = ui_colors(theme);
        let accent = colors.primary;
        let hover = colors.surface_container_high;
        let surface = colors.surface_container;

        button::Style {
            background: Some(Background::Color(if selected {
                Color { a: 0.18, ..accent }
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                hover
            } else {
                surface
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
                width: 1.0,
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
    let mode = pick_list(
        BatchRenameExtensionMode::options(),
        Some(state.extension.mode),
        |mode| Message::BatchRename(BatchRenameMessage::ExtensionModeSelected(mode)),
    )
    .width(Length::Fill)
    .text_size(12)
    .padding([5, 8]);
    let controls = column![section_title("Extension")];
    let controls = if state.extension.mode == BatchRenameExtensionMode::Replace {
        controls.push(
            row![
                mode,
                input_column(
                    "New extension",
                    &state.extension.replacement,
                    BatchRenameMessage::ExtensionReplacementChanged
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
    } else {
        controls.push(mode)
    };

    controls.spacing(6).into()
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
                "Step",
                &state.sequence.step_input,
                BatchRenameMessage::SequenceStepChanged
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
                .label(crate::localization::translate_current("Original stem"))
                .on_toggle(|value| Message::BatchRename(
                    BatchRenameMessage::SequenceIncludeOriginalToggled(value)
                )),
            checkbox(state.sequence.preserve_extension)
                .label(crate::localization::translate_current("Extension"))
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
    let mut controls = column![
        section_title("Replace"),
        pick_list(
            BatchRenameReplaceScope::options(),
            Some(state.replace.scope),
            |scope| { Message::BatchRename(BatchRenameMessage::ReplaceScopeSelected(scope)) }
        )
        .width(Length::Fill)
        .text_size(12)
        .padding([5, 8]),
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
    ];
    if state.replace.scope == BatchRenameReplaceScope::Range {
        controls = controls.push(
            row![
                input_column(
                    "Range start",
                    &state.replace.range_start_input,
                    BatchRenameMessage::ReplaceRangeStartChanged
                ),
                input_column(
                    "Range length",
                    &state.replace.range_length_input,
                    BatchRenameMessage::ReplaceRangeLengthChanged
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }

    controls
        .push(
            checkbox(state.replace.ignore_case)
                .label(crate::localization::translate_current("Ignore case"))
                .on_toggle(|value| {
                    Message::BatchRename(BatchRenameMessage::ReplaceIgnoreCaseToggled(value))
                }),
        )
        .spacing(6)
        .into()
}

fn insert_controls(state: &BatchRenameState) -> Element<'_, Message> {
    let mut fields = row![input_column(
        "Text",
        &state.insert.text,
        BatchRenameMessage::InsertTextChanged
    )]
    .spacing(8)
    .align_y(Alignment::Center);
    match state.insert.mode {
        BatchRenameInsertMode::Position => {
            fields = fields.push(input_column(
                "Position",
                &state.insert.position_input,
                BatchRenameMessage::InsertPositionChanged,
            ));
        }
        BatchRenameInsertMode::AfterAnchor => {
            fields = fields.push(input_column(
                "After text",
                &state.insert.anchor,
                BatchRenameMessage::InsertAnchorChanged,
            ));
        }
        BatchRenameInsertMode::Before | BatchRenameInsertMode::After => {}
    }

    let mut controls = column![
        section_title("Insert"),
        pick_list(
            BatchRenameInsertMode::options(),
            Some(state.insert.mode),
            |mode| { Message::BatchRename(BatchRenameMessage::InsertModeSelected(mode)) }
        )
        .width(Length::Fill)
        .text_size(12)
        .padding([5, 8]),
        fields,
    ];
    if state.insert.mode != BatchRenameInsertMode::Before {
        controls = controls.push(
            checkbox(state.insert.ignore_extension)
                .label(crate::localization::translate_current("Ignore extension"))
                .on_toggle(|value| {
                    Message::BatchRename(BatchRenameMessage::InsertIgnoreExtensionToggled(value))
                }),
        );
    }

    controls.spacing(6).into()
}

fn slice_controls(state: &BatchRenameState) -> Element<'_, Message> {
    let location = match state.slice.mode {
        BatchRenameSliceMode::Position => input_column(
            "Start",
            &state.slice.start_input,
            BatchRenameMessage::SliceStartChanged,
        ),
        BatchRenameSliceMode::AfterAnchor => input_column(
            "After text",
            &state.slice.anchor,
            BatchRenameMessage::SliceAnchorChanged,
        ),
    };

    column![
        section_title("Slice"),
        pick_list(
            BatchRenameSliceMode::options(),
            Some(state.slice.mode),
            |mode| { Message::BatchRename(BatchRenameMessage::SliceModeSelected(mode)) }
        )
        .width(Length::Fill)
        .text_size(12)
        .padding([5, 8]),
        row![
            location,
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
    let mode_controls: Element<'_, Message> = match state.remove.mode {
        BatchRenameRemoveMode::TextAndRange => column![
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
        .into(),
        BatchRenameRemoveMode::CharacterClasses => column![
            row![
                remove_class_toggle(state, BatchRenameRemoveClass::Lowercase),
                remove_class_toggle(state, BatchRenameRemoveClass::Uppercase),
                remove_class_toggle(state, BatchRenameRemoveClass::Digits),
                remove_class_toggle(state, BatchRenameRemoveClass::Symbols),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row![
                remove_class_toggle(state, BatchRenameRemoveClass::Brackets),
                remove_class_toggle(state, BatchRenameRemoveClass::Whitespace),
                remove_class_toggle(state, BatchRenameRemoveClass::Hanzi),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(6)
        .into(),
    };

    column![
        section_title("Remove"),
        pick_list(
            BatchRenameRemoveMode::options(),
            Some(state.remove.mode),
            |mode| { Message::BatchRename(BatchRenameMessage::RemoveModeSelected(mode)) }
        )
        .width(Length::Fill)
        .text_size(12)
        .padding([5, 8]),
        mode_controls,
    ]
    .spacing(6)
    .into()
}

fn remove_class_toggle(
    state: &BatchRenameState,
    class: BatchRenameRemoveClass,
) -> Element<'static, Message> {
    checkbox(state.remove.classes.contains(&class))
        .label(class.to_string())
        .on_toggle(move |value| {
            Message::BatchRename(BatchRenameMessage::RemoveClassToggled(class, value))
        })
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

fn case_controls(state: &BatchRenameState) -> Element<'_, Message> {
    column![
        section_title("Case"),
        pick_list(BatchRenameCaseRule::options(), Some(state.case), |case| {
            Message::BatchRename(BatchRenameMessage::CaseSelected(case))
        })
        .width(Length::Fill)
        .text_size(12)
        .padding([5, 8]),
    ]
    .spacing(6)
    .into()
}

fn preview_rows(
    state: &BatchRenameState,
    scrollbar_visibility: ScrollbarVisibility,
    scrollbar_viewport: Option<ScrollbarViewport>,
) -> Element<'_, Message> {
    let mut rows = Column::new().spacing(4);
    let dragging_active = state.dragging_preview_source().is_some();
    let editing_source = state.editing_target_name_source();
    let editing_input = state.editing_target_name_input();
    for row in &state.preview.rows {
        let is_dragging = state.dragging_preview_source() == Some(row.source.as_path());
        let is_editing = editing_source == Some(row.source.as_path());
        rows = rows.push(preview_row(
            row,
            is_dragging,
            dragging_active,
            is_editing,
            editing_input,
        ));
    }

    let scroll_region = ScrollbarRegion::BatchRenamePreview;
    let scroller = scrollable(smooth_scroll_content(rows, scroll_region.clone()))
        .id(smooth_scroll_id(&scroll_region))
        .direction(enhanced_vertical_scrollbar_direction(
            scrollbar_visibility,
            6.0,
        ))
        .style(enhanced_scrollbar_style(scrollbar_visibility))
        .height(Length::Fixed(BATCH_RENAME_PREVIEW_HEIGHT))
        .on_scroll(scrollbar_on_scroll(scroll_region.clone(), |_| {
            Message::BatchRenamePreviewScrolled
        }));
    let scroller = enhanced_scrollbar(
        scroller,
        scrollbar_visibility,
        scrollbar_viewport,
        ScrollbarAxis::Vertical,
        6.0,
    );

    column![section_title("Preview"), scroller]
        .spacing(6)
        .into()
}

fn preview_row<'a>(
    row_state: &'a BatchRenamePreviewRow,
    is_dragging: bool,
    dragging_active: bool,
    is_editing: bool,
    editing_input: &'a str,
) -> Element<'a, Message> {
    let source =
        format_middle_ellipsized_text(&row_state.original_name, BATCH_RENAME_PATH_MAX_CHARS);
    let target = format_middle_ellipsized_text(&row_state.target_name, BATCH_RENAME_PATH_MAX_CHARS);
    let status = row_state.status.label();
    let source_cell: Element<'a, Message> = mouse_area(
        container(readable_text(source).size(12))
            .width(Length::FillPortion(3))
            .padding([1, 0]),
    )
    .on_press(Message::BatchRename(
        BatchRenameMessage::PreviewDragStarted(row_state.source.clone()),
    ))
    .interaction(iced::mouse::Interaction::Grab)
    .into();
    let target_cell: Element<'a, Message> = if is_editing {
        text_input(
            &crate::localization::translate_current("New name"),
            editing_input,
        )
        .id(batch_rename_preview_name_input_id(&row_state.source))
        .on_input(|value| Message::BatchRename(BatchRenameMessage::PreviewNameChanged(value)))
        .on_submit(Message::BatchRename(
            BatchRenameMessage::PreviewNameEditCommitted,
        ))
        .padding([5, 6])
        .size(12)
        .width(Length::FillPortion(3))
        .into()
    } else {
        mouse_area(
            container(readable_text(target).size(12))
                .width(Length::FillPortion(3))
                .padding([1, 0]),
        )
        .on_press(Message::BatchRename(
            BatchRenameMessage::PreviewNameEditStarted(row_state.source.clone()),
        ))
        .interaction(iced::mouse::Interaction::Text)
        .into()
    };
    let row_container = container(
        row![
            source_cell,
            target_cell,
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

fn mode_selector(state: &BatchRenameState) -> Element<'static, Message> {
    segmented_choice_row(vec![
        SegmentedChoice {
            label: "Simple",
            selected: state.simple_mode,
            message: Message::BatchRename(BatchRenameMessage::SimpleModeSelected(true)),
        },
        SegmentedChoice {
            label: "Advanced",
            selected: !state.simple_mode,
            message: Message::BatchRename(BatchRenameMessage::SimpleModeSelected(false)),
        },
    ])
}

fn simple_controls(state: &BatchRenameState) -> Element<'_, Message> {
    let kind_selector = container(
        row![
            simple_kind_radio(
                BatchRenameSimpleKind::Template,
                state.simple.kind,
                "Rename using a template",
            ),
            simple_kind_radio(
                BatchRenameSimpleKind::ReplaceText,
                state.simple.kind,
                "Find and replace text",
            ),
        ]
        .spacing(24),
    )
    .width(Length::Fill)
    .center_x(Length::Fill);

    let controls = match state.simple.kind {
        BatchRenameSimpleKind::Template => simple_template_controls(state),
        BatchRenameSimpleKind::ReplaceText => simple_replace_controls(state),
    };

    column![kind_selector, controls]
        .spacing(12)
        .width(Length::Fill)
        .into()
}

fn simple_kind_radio(
    kind: BatchRenameSimpleKind,
    selected: BatchRenameSimpleKind,
    label: &'static str,
) -> Element<'static, Message> {
    let is_selected = kind == selected;
    let choice = button(
        row![radio_dot(is_selected), readable_text(label).size(12)]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .padding([4, 6])
    .style(|theme, status| {
        let background = matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then(|| Background::Color(ui_colors(theme).surface_container_high));
        button::Style {
            background,
            text_color: base_text_color(theme),
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    });

    if is_selected {
        choice.into()
    } else {
        choice
            .on_press(Message::BatchRename(
                BatchRenameMessage::SimpleKindSelected(kind),
            ))
            .into()
    }
}

fn radio_dot(selected: bool) -> Element<'static, Message> {
    container(
        Space::new()
            .width(Length::Fixed(6.0))
            .height(Length::Fixed(6.0)),
    )
    .width(Length::Fixed(14.0))
    .height(Length::Fixed(14.0))
    .center_x(Length::Fixed(14.0))
    .center_y(Length::Fixed(14.0))
    .style(move |theme| {
        let accent = ui_colors(theme).primary;
        container::Style {
            background: selected.then(|| Background::Color(accent)),
            border: Border {
                color: if selected {
                    accent
                } else {
                    subtle_border_color(theme)
                },
                width: 1.5,
                radius: 7.0.into(),
            },
            ..container::Style::default()
        }
    })
    .into()
}

fn simple_template_controls(state: &BatchRenameState) -> Element<'_, Message> {
    let add_button = secondary_action_button(
        "Add",
        Message::BatchRename(BatchRenameMessage::SimpleTokenMenuToggled),
    );
    let template_input = text_input(
        &crate::localization::translate_current("Original name"),
        &state.simple.template,
    )
    .on_input(|value| Message::BatchRename(BatchRenameMessage::SimpleTemplateChanged(value)))
    .padding([6, 8])
    .size(13)
    .width(Length::Fill);

    row![
        template_input,
        anchored_popup(
            add_button,
            state.simple.token_menu_open.then(simple_token_menu),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn simple_token_menu() -> Element<'static, Message> {
    let mut items = Column::new().spacing(2);
    for token in BatchRenameSimpleToken::ALL {
        items = items.push(
            button(readable_text(token.label()).size(12))
                .padding([5, 10])
                .width(Length::Fill)
                .style(context_menu_item_button_style())
                .on_press(Message::BatchRename(
                    BatchRenameMessage::SimpleTokenSelected(token),
                )),
        );
    }

    container(items)
        .padding(4)
        .width(Length::Fixed(230.0))
        .style(context_menu_style)
        .into()
}

fn simple_replace_controls(state: &BatchRenameState) -> Element<'_, Message> {
    row![
        input_column(
            "Find",
            &state.simple.find,
            BatchRenameMessage::SimpleFindChanged
        ),
        input_column(
            "With",
            &state.simple.replacement,
            BatchRenameMessage::SimpleReplacementChanged,
        ),
    ]
    .spacing(8)
    .into()
}

fn input_column<'a>(
    label: &'static str,
    value: &'a str,
    message: fn(String) -> BatchRenameMessage,
) -> Element<'a, Message> {
    column![
        readable_text(label).size(11),
        text_input(&crate::localization::translate_current(label), value)
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
