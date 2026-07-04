use std::path::{Path, PathBuf};

use iced::widget::{column, container, mouse_area, row, text_input, Column};
use iced::{Alignment, Element, Length};

use crate::anchored_popup::anchored_popup;
use crate::app::search_index_settings::{
    search_index_display_path, search_index_exclude_pattern_display_path,
};
use crate::app::FileBrowser;
use crate::appearance::{
    path_suggestion_item_style, path_suggestions_style, selected_path_suggestion_item_style,
};
use crate::formatting::format_middle_ellipsized_text;
use crate::model::{
    Message, SearchIndexPathRuleEditMode, SearchIndexPathRuleKind, SearchIndexPathRuleSelection,
    SearchIndexSettingsSection,
};
use crate::typography::readable_text;

use super::{action_button, section_panel, PATH_RULE_MAX_CHARS};

pub(super) fn search_index_path_rules_content(browser: &FileBrowser) -> Column<'_, Message> {
    column![
        path_rules_header(),
        path_rule_section(
            browser,
            SearchIndexPathRuleKind::Indexed,
            "Indexed paths",
            Some("Choose folders under your home directory to build indexes for."),
            "Add index path",
            Message::SearchIndexIndexedPathAddRequested,
        ),
        path_rule_section(
            browser,
            SearchIndexPathRuleKind::Excluded,
            "Exclude rules",
            Some(
                "Use node_modules/ or *.log for global rules. Use ~/... or an absolute path for path-based excludes.",
            ),
            "Add exclude rule",
            Message::SearchIndexExcludeRuleAddRequested,
        ),
    ]
    .spacing(12)
    .width(Length::Fill)
}

fn path_rules_header() -> Element<'static, Message> {
    row![
        readable_text("Path rules").size(14).width(Length::Fill),
        action_button(
            "Back",
            Some(Message::SearchIndexSettingsSectionSelected(
                SearchIndexSettingsSection::Overview,
            )),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn path_rule_section<'a>(
    browser: &'a FileBrowser,
    kind: SearchIndexPathRuleKind,
    title: &'static str,
    description: Option<&'static str>,
    add_label: &'static str,
    add_message: Message,
) -> Element<'a, Message> {
    let home = browser.search_index_home_directory();
    let entries = browser
        .search_index
        .path_rule_entries()
        .into_iter()
        .filter(|entry| entry.kind == kind)
        .collect::<Vec<_>>();
    let show_editor = section_shows_path_rule_editor(browser, kind);

    let mut content = column![row![
        readable_text(title).size(13).width(Length::Fill),
        action_button(add_label, Some(add_message)),
    ]
    .spacing(8)
    .align_y(Alignment::Center),];

    if let Some(description) = description {
        content = content.push(readable_text(description).size(12));
    }
    if show_editor {
        content = content.push(path_rule_editor_panel(browser, kind));
    }

    if entries.is_empty() {
        let empty_message = match kind {
            SearchIndexPathRuleKind::Indexed => "No indexed paths configured.",
            SearchIndexPathRuleKind::Excluded => "No exclude rules configured.",
        };
        content = content.push(readable_text(empty_message).size(12));
    } else {
        for entry in entries {
            if let Some(label) = path_rule_label(browser, &entry.selection, &home) {
                content = content.push(path_rule_row(browser, entry.selection, label));
            }
        }
    }

    section_panel(content)
}

fn section_shows_path_rule_editor(browser: &FileBrowser, kind: SearchIndexPathRuleKind) -> bool {
    match &browser.search_index.path_rule_editor {
        Some(SearchIndexPathRuleEditMode::Adding) => browser.search_index.path_rule_kind == kind,
        Some(SearchIndexPathRuleEditMode::Modifying(selection)) => {
            path_rule_selection_kind(selection) == kind
        }
        None => false,
    }
}

fn path_rule_label(
    browser: &FileBrowser,
    selection: &SearchIndexPathRuleSelection,
    home: &Path,
) -> Option<String> {
    match selection {
        SearchIndexPathRuleSelection::IndexedRoot(root) => {
            Some(search_index_display_path(root, home))
        }
        SearchIndexPathRuleSelection::ExcludePattern(index) => browser
            .search_index
            .exclude_pattern_inputs
            .get(*index)
            .map(|pattern| {
                search_index_exclude_pattern_display_path(
                    &browser.search_index.profile_roots,
                    pattern,
                    home,
                )
                .unwrap_or_else(|| pattern.clone())
            }),
    }
}

fn path_rule_row(
    browser: &FileBrowser,
    selection: SearchIndexPathRuleSelection,
    label: String,
) -> Element<'static, Message> {
    let is_selected = browser.search_index.selected_path_rule.as_ref() == Some(&selection);
    let label = format_middle_ellipsized_text(&label, PATH_RULE_MAX_CHARS);
    let row = row![
        readable_text(label).size(12).width(Length::Fill),
        action_button(
            "Edit",
            Some(Message::SearchIndexPathRuleEditRequested(selection.clone())),
        ),
        action_button(
            "Remove",
            Some(Message::SearchIndexPathRuleRemoveRequested(
                selection.clone()
            )),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let item = container(row).padding([5, 8]).width(Length::Fill);
    let item = if is_selected {
        item.style(selected_path_suggestion_item_style)
    } else {
        item.style(path_suggestion_item_style)
    };

    mouse_area(item)
        .on_press(Message::SearchIndexPathRuleSelected(selection))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

fn path_rule_editor_panel(
    browser: &FileBrowser,
    kind: SearchIndexPathRuleKind,
) -> Element<'_, Message> {
    let title = match (&browser.search_index.path_rule_editor, kind) {
        (Some(SearchIndexPathRuleEditMode::Adding), SearchIndexPathRuleKind::Indexed) => {
            "Add index path"
        }
        (Some(SearchIndexPathRuleEditMode::Adding), SearchIndexPathRuleKind::Excluded) => {
            "Add exclude rule"
        }
        (Some(SearchIndexPathRuleEditMode::Modifying(_)), SearchIndexPathRuleKind::Indexed) => {
            "Edit index path"
        }
        (Some(SearchIndexPathRuleEditMode::Modifying(_)), SearchIndexPathRuleKind::Excluded) => {
            "Edit exclude rule"
        }
        (None, _) => "",
    };
    let placeholder = match kind {
        SearchIndexPathRuleKind::Indexed => "~/Projects",
        SearchIndexPathRuleKind::Excluded => "node_modules/ or ~/Projects/target",
    };
    let input = text_input(placeholder, &browser.search_index.path_rule_input)
        .on_input(Message::SearchIndexPathRuleInputChanged)
        .on_submit(Message::SearchIndexPathRuleEditorCommitted)
        .padding([6, 8])
        .size(13)
        .width(Length::Fill);
    let popup = (!browser.search_index.path_rule_suggestions.is_empty())
        .then(|| path_rule_suggestions_popup(browser));
    let save_message = path_rule_editor_save_message(browser);

    let mut content = column![
        readable_text(title).size(12),
        container(anchored_popup(input, popup)).width(Length::Fill),
    ];

    if let Some(error) = &browser.search_index.path_rule_error {
        content = content.push(readable_text(error.clone()).size(12));
    }

    content = content.push(
        row![
            action_button("Cancel", Some(Message::SearchIndexPathRuleEditCanceled),),
            action_button("Save", save_message),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );

    container(content.spacing(8))
        .padding([6, 8])
        .width(Length::Fill)
        .style(path_suggestion_item_style)
        .into()
}

fn path_rule_editor_save_message(browser: &FileBrowser) -> Option<Message> {
    if !browser.search_index_path_input_can_apply() {
        return None;
    }

    match browser.search_index.path_rule_editor {
        Some(SearchIndexPathRuleEditMode::Adding) => Some(Message::SearchIndexPathRuleAdded),
        Some(SearchIndexPathRuleEditMode::Modifying(_)) => {
            Some(Message::SearchIndexPathRuleUpdated)
        }
        None => None,
    }
}

fn path_rule_suggestions_popup(browser: &FileBrowser) -> Element<'_, Message> {
    let home = browser.search_index_home_directory();
    let mut suggestions = Column::new().spacing(3).padding(4);

    for suggestion in &browser.search_index.path_rule_suggestions {
        suggestions = suggestions.push(path_rule_suggestion_row(&home, suggestion));
    }

    container(suggestions)
        .width(Length::Fill)
        .style(path_suggestions_style)
        .into()
}

fn path_rule_suggestion_row(home: &Path, suggestion: &PathBuf) -> Element<'static, Message> {
    let label = search_index_display_path(suggestion, home);
    let label = format_middle_ellipsized_text(&label, PATH_RULE_MAX_CHARS);
    let item = container(readable_text(label).size(12).width(Length::Fill))
        .padding([5, 8])
        .width(Length::Fill)
        .style(path_suggestion_item_style);

    mouse_area(item)
        .on_press(Message::SearchIndexPathRuleSuggestionSelected(
            suggestion.to_path_buf(),
        ))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

fn path_rule_selection_kind(selection: &SearchIndexPathRuleSelection) -> SearchIndexPathRuleKind {
    match selection {
        SearchIndexPathRuleSelection::IndexedRoot(_) => SearchIndexPathRuleKind::Indexed,
        SearchIndexPathRuleSelection::ExcludePattern(_) => SearchIndexPathRuleKind::Excluded,
    }
}
