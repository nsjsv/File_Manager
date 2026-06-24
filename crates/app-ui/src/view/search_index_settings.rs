use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use file_index::{
    DirectoryErrorPolicy, FileSearchIndexFailure, FileSearchIndexMode, FileSearchIndexStatus,
    MediaMetadataScope,
};
use iced::widget::{
    button, column, container, mouse_area, radio, row, text, text_input, Button, Column, Space,
    Text,
};
use iced::{Alignment, Element, Length, Theme};

use crate::app::search_index_settings::{
    search_index_display_path, search_index_exclude_pattern_display_path,
};
use crate::app::FileBrowser;
use crate::appearance::{
    context_menu_button_style, path_suggestion_item_style, selected_path_suggestion_item_style,
};
use crate::config::SearchBackendMode;
use crate::formatting::{format_file_size, format_middle_ellipsized_text, format_system_time};
use crate::model::{
    Message, SearchIndexDaemonStatus, SearchIndexPathRuleEditMode, SearchIndexPathRuleKind,
    SearchIndexPathRuleSelection,
};
use crate::typography::readable_text;

use super::option_controls::selectable_choice_row;
use super::toggle_switch::switch_control;

const ROOT_PATH_MAX_CHARS: usize = 72;
const INDEX_DIR_MAX_CHARS: usize = 76;
const FAILURE_PATH_MAX_CHARS: usize = 78;
const PATH_RULE_MAX_CHARS: usize = 84;

pub(super) fn search_index_settings_content(browser: &FileBrowser) -> Column<'_, Message> {
    let mut content = column![settings_header(), search_mode_panel(browser),]
        .spacing(12)
        .width(Length::Fill);

    if browser.search_backend_mode() == SearchBackendMode::Indexed {
        content = content
            .push(profile_policy_panel(browser))
            .push(index_directory_panel(browser))
            .push(path_rules_panel(browser))
            .push(root_statuses_panel(browser));
    }

    content
}

fn settings_header() -> Element<'static, Message> {
    row![
        readable_text("Search Index").size(20).width(Length::Fill),
        action_button("Refresh", Some(Message::SearchIndexStatusRefreshRequested),),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

fn index_directory_panel(browser: &FileBrowser) -> Element<'_, Message> {
    let directory = browser.search_index.base_dir.to_string_lossy();
    let directory = format_middle_ellipsized_text(directory.as_ref(), INDEX_DIR_MAX_CHARS);
    section_panel(column![
        readable_text("Index directory").size(13),
        readable_text(directory).size(12),
    ])
}

fn search_mode_panel(browser: &FileBrowser) -> Element<'static, Message> {
    let mode = browser.search_backend_mode();
    section_panel(column![
        readable_text("Search mode").size(13),
        selectable_choice_row(
            "Simple search",
            "Live filename and path search. No index is built or maintained.",
            mode == SearchBackendMode::Simple,
            Message::SearchBackendModeSelected(SearchBackendMode::Simple),
        ),
        selectable_choice_row(
            "Indexed search",
            "Uses configured indexed paths and enables content or media indexing options.",
            mode == SearchBackendMode::Indexed,
            Message::SearchBackendModeSelected(SearchBackendMode::Indexed),
        ),
    ])
}

fn profile_policy_panel<'a>(browser: &'a FileBrowser) -> Element<'a, Message> {
    let directory_error_state = match browser.search_index.directory_error_policy {
        DirectoryErrorPolicy::SkipUnreadable => "Skip and record",
        DirectoryErrorPolicy::Abort => "Abort scan",
    };
    let roots_label = if browser.search_index.profile_roots.is_empty() {
        "No roots selected".to_owned()
    } else {
        format!(
            "{} explicit path(s)",
            browser.search_index.profile_roots.len()
        )
    };
    let maintenance_state = if browser.search_index.maintenance_paused {
        "Paused"
    } else if browser.search_index.profile_roots.is_empty() {
        "No roots"
    } else {
        "Running"
    };
    let daemon_state = search_index_daemon_status_label(
        browser.search_index.daemon_status.as_ref(),
        browser.search_index.daemon_status_loading,
    );
    let daemon_controls_enabled = !browser.search_index.daemon_status_loading;
    let mut profile = column![
        readable_text("Profile").size(13),
        metadata_row("Service", daemon_state),
        metadata_row("Profile id", browser.search_index.profile_id.clone()),
        metadata_row("Roots", roots_label),
        metadata_row("Maintenance", maintenance_state.to_owned()),
        row![
            action_button(
                "Restart service",
                daemon_controls_enabled.then_some(Message::SearchIndexDaemonRestartRequested),
            ),
            action_button(
                if browser.search_index.maintenance_paused {
                    "Resume"
                } else {
                    "Pause"
                },
                Some(Message::SearchIndexMaintenancePauseToggled),
            ),
            action_button(
                "Delete profile",
                browser
                    .search_index
                    .has_active_profile_roots()
                    .then_some(Message::SearchIndexProfileDeleteRequested),
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        metadata_row("Files", "Filename and path catalog".to_owned()),
        directory_error_policy_row(
            directory_error_state,
            browser.search_index.directory_error_policy,
        ),
        profile_switch_row(
            "Contents",
            browser.search_index.content_index_enabled,
            Message::SearchIndexContentEnabledToggled(!browser.search_index.content_index_enabled),
        ),
        media_scope_row(
            "No media metadata",
            "Filename search only for images, audio, and video.",
            browser.search_index.media_metadata_scope == MediaMetadataScope::Off,
            MediaMetadataScope::Off,
        ),
        media_scope_row(
            "Image metadata",
            "Image dimensions and EXIF, without audio or video probing.",
            browser.search_index.media_metadata_scope == MediaMetadataScope::Images,
            MediaMetadataScope::Images,
        ),
        media_scope_row(
            "All media metadata",
            "Image metadata plus audio and video metadata.",
            browser.search_index.media_metadata_scope == MediaMetadataScope::All,
            MediaMetadataScope::All,
        ),
    ];

    if let Some(error) = &browser.search_index.profile_error {
        profile = profile.push(readable_text(format!("Profile error: {error}")).size(12));
    }
    profile = profile.push(
        readable_text("Changing content or media indexing applies to future rebuilds and updates.")
            .size(12),
    );

    section_panel(profile)
}

fn search_index_daemon_status_label(
    status: Option<&SearchIndexDaemonStatus>,
    loading: bool,
) -> String {
    if loading {
        return "Checking...".to_owned();
    }
    match status {
        Some(SearchIndexDaemonStatus::Reachable) => "Connected".to_owned(),
        Some(SearchIndexDaemonStatus::Unreachable(error)) => {
            format!("Unavailable: {error}")
        }
        None => "Unknown".to_owned(),
    }
}

fn directory_error_policy_row(
    state: &'static str,
    selected: DirectoryErrorPolicy,
) -> Element<'static, Message> {
    row![
        metadata_row("Unreadable directories", state.to_owned()),
        action_button(
            "Skip",
            (selected != DirectoryErrorPolicy::SkipUnreadable).then_some(
                Message::SearchIndexDirectoryErrorPolicySelected(
                    DirectoryErrorPolicy::SkipUnreadable,
                ),
            ),
        ),
        action_button(
            "Abort",
            (selected != DirectoryErrorPolicy::Abort).then_some(
                Message::SearchIndexDirectoryErrorPolicySelected(DirectoryErrorPolicy::Abort),
            ),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn media_scope_row(
    title: &'static str,
    description: &'static str,
    selected: bool,
    scope: MediaMetadataScope,
) -> Element<'static, Message> {
    selectable_choice_row(
        title,
        description,
        selected,
        Message::SearchIndexMediaScopeSelected(scope),
    )
}

fn profile_switch_row(
    label: &'static str,
    enabled: bool,
    message: Message,
) -> Element<'static, Message> {
    let content = row![
        readable_text(label).size(12).width(Length::Fill),
        switch_control(enabled),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    button(container(content).padding([5, 8]).width(Length::Fill))
        .on_press(message)
        .width(Length::Fill)
        .style(context_menu_button_style())
        .into()
}

fn path_rules_panel<'a>(browser: &'a FileBrowser) -> Element<'a, Message> {
    let home = browser.search_index_home_directory();
    let mut rules: Column<'a, Message> = Column::new()
        .spacing(8)
        .push(scoped_text("Path rules").size(13))
        .push(path_rules_header());

    let path_rule_entries = browser.search_index.path_rule_entries();
    let has_rows = !path_rule_entries.is_empty();
    for entry in path_rule_entries {
        let Some(label) = path_rule_label(browser, &entry.selection, &home) else {
            continue;
        };
        rules = rules.push(path_rule_content(
            browser,
            entry.kind,
            entry.selection,
            label,
        ));
    }

    if !has_rows {
        rules = rules.push(scoped_text("No path rules configured.").size(12));
    }

    if matches!(
        browser.search_index.path_rule_editor,
        Some(SearchIndexPathRuleEditMode::Adding)
    ) {
        rules = rules.push(path_rule_editor(
            browser.search_index.path_rule_kind,
            &browser.search_index.path_rule_input,
        ));
    }

    let add_message = (!matches!(
        browser.search_index.path_rule_editor,
        Some(SearchIndexPathRuleEditMode::Adding)
    ) || browser.search_index_path_input_can_apply())
    .then_some(Message::SearchIndexPathRuleAdded);
    let selected_message = browser
        .selected_search_index_path_rule_exists()
        .then_some(Message::SearchIndexPathRuleRemoved);
    let update_message = (browser.selected_search_index_path_rule_exists()
        && (!matches!(
            browser.search_index.path_rule_editor,
            Some(SearchIndexPathRuleEditMode::Modifying(_))
        ) || browser.search_index_path_input_can_apply()))
    .then_some(Message::SearchIndexPathRuleUpdated);

    rules = rules.push(
        row![
            action_button("Add", add_message),
            action_button("Remove", selected_message),
            action_button("Modify", update_message),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );

    mouse_area(section_panel(rules))
        .on_press(Message::SearchIndexPathRuleEditorCommitted)
        .into()
}

fn path_rule_label(
    browser: &FileBrowser,
    selection: &SearchIndexPathRuleSelection,
    home: &std::path::Path,
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

fn path_rules_header<'a>() -> Element<'a, Message> {
    let content = row![
        scoped_text("Index").size(12).width(Length::Fixed(72.0)),
        scoped_text("Exclude").size(12).width(Length::Fixed(72.0)),
        scoped_text("Path").size(12).width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    container(content)
        .padding([0, 8])
        .width(Length::Fill)
        .into()
}

fn path_rule_content<'a>(
    browser: &'a FileBrowser,
    kind: SearchIndexPathRuleKind,
    selection: SearchIndexPathRuleSelection,
    path: String,
) -> Element<'a, Message> {
    if browser
        .search_index
        .path_rule_editor
        .as_ref()
        .is_some_and(|editor| *editor == SearchIndexPathRuleEditMode::Modifying(selection.clone()))
    {
        return path_rule_editor(
            browser.search_index.path_rule_kind,
            &browser.search_index.path_rule_input,
        );
    }
    path_rule_row(
        kind,
        selection,
        path,
        &browser.search_index.selected_path_rule,
    )
}

fn path_rule_row(
    kind: SearchIndexPathRuleKind,
    selection: SearchIndexPathRuleSelection,
    path: String,
    selected: &Option<SearchIndexPathRuleSelection>,
) -> Element<'static, Message> {
    let is_selected = selected.as_ref() == Some(&selection);
    let path = format_middle_ellipsized_text(&path, PATH_RULE_MAX_CHARS);
    let content = row![
        path_rule_radio(SearchIndexPathRuleKind::Indexed, kind, selection.clone(),),
        path_rule_radio(SearchIndexPathRuleKind::Excluded, kind, selection.clone(),),
        readable_text(path).size(12).width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let row_container = container(content).padding([5, 8]).width(Length::Fill);
    let row_container = if is_selected {
        row_container.style(selected_path_suggestion_item_style)
    } else {
        row_container.style(path_suggestion_item_style)
    };
    mouse_area(row_container)
        .on_press(Message::SearchIndexPathRuleSelected(selection))
        .into()
}

fn path_rule_radio(
    column_kind: SearchIndexPathRuleKind,
    row_kind: SearchIndexPathRuleKind,
    selection: SearchIndexPathRuleSelection,
) -> Element<'static, Message> {
    let message_selection = selection.clone();
    radio("", column_kind, Some(row_kind), move |target_kind| {
        Message::SearchIndexPathRuleKindChanged(message_selection.clone(), target_kind)
    })
    .size(14.0)
    .spacing(0.0)
    .width(Length::Fixed(72.0))
    .into()
}

fn path_rule_editor<'a>(kind: SearchIndexPathRuleKind, input: &'a str) -> Element<'a, Message> {
    let content = row![
        path_rule_kind_radio(SearchIndexPathRuleKind::Indexed, kind),
        path_rule_kind_radio(SearchIndexPathRuleKind::Excluded, kind),
        text_input("~", &input)
            .on_input(Message::SearchIndexPathRuleInputChanged)
            .padding([6, 8])
            .size(13)
            .width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    container(content)
        .padding([0, 8])
        .width(Length::Fill)
        .into()
}

fn path_rule_kind_radio(
    kind: SearchIndexPathRuleKind,
    selected: SearchIndexPathRuleKind,
) -> Element<'static, Message> {
    radio(
        "",
        kind,
        Some(selected),
        Message::SearchIndexPathRuleKindSelected,
    )
    .size(14.0)
    .spacing(0.0)
    .width(Length::Fixed(72.0))
    .into()
}

fn root_statuses_panel<'a>(browser: &'a FileBrowser) -> Element<'a, Message> {
    let roots = browser.search_index_setting_roots();
    let mut statuses: Column<'a, Message> = Column::new()
        .spacing(8)
        .push(scoped_text("Indexed roots").size(13));

    if roots.is_empty() {
        statuses = statuses.push(scoped_text("No searchable roots are available yet.").size(12));
    } else {
        for root in roots {
            statuses = statuses.push(root_status_card(browser, root));
        }
    }

    section_panel(statuses)
}

fn root_status_card<'a>(browser: &'a FileBrowser, root: PathBuf) -> Element<'a, Message> {
    let is_loading = browser
        .search_index
        .status_loading_roots
        .contains_key(&root);
    let is_indexing = browser.search_index.indexing_roots.contains(&root);
    let status = browser.search_index.statuses.get(&root);
    let error = browser.search_index.root_errors.get(&root);
    let index_dir = file_index::search_index_dir_for_root(&browser.search_index.base_dir, &root);

    let root_label = root.to_string_lossy();
    let root_label = format_middle_ellipsized_text(root_label.as_ref(), ROOT_PATH_MAX_CHARS);
    let index_dir_label = index_dir.to_string_lossy();
    let index_dir_label =
        format_middle_ellipsized_text(index_dir_label.as_ref(), INDEX_DIR_MAX_CHARS);

    let mut details = Column::new()
        .spacing(6)
        .push(readable_text(root_label).size(13))
        .push(metadata_row("Index path", index_dir_label));

    if is_indexing {
        details = details.push(readable_text("Indexing is queued or running.").size(12));
    } else if is_loading {
        details = details.push(readable_text("Loading index status...").size(12));
    }

    if let Some(error) = error {
        details = details.push(readable_text(format!("Index error: {error}")).size(12));
    }

    details = match status {
        Some(status) => details
            .push(index_status_rows(status))
            .push(root_action_row(
                root.clone(),
                is_loading || is_indexing,
                status.exists,
                !status.failures.is_empty(),
            ))
            .push(failures_panel(status)),
        None => details.push(root_action_row_without_status(
            root,
            is_loading || is_indexing,
        )),
    };

    container(details)
        .padding(10)
        .width(Length::Fill)
        .style(path_suggestion_item_style)
        .into()
}

fn index_status_rows(status: &FileSearchIndexStatus) -> Element<'static, Message> {
    let state_label = if status.stale {
        "Needs rebuild"
    } else if status.exists {
        "Present"
    } else {
        "Missing"
    };
    let last_update = status
        .updated_at_ms
        .or(status.built_at_ms)
        .map(format_unix_ms)
        .unwrap_or_else(|| "Never".to_owned());
    let mut rows = column![
        metadata_row("State", state_label.to_owned()),
        metadata_row("Records", status.record_count.to_string()),
        metadata_row("Size", format_file_size(status.index_size_bytes)),
        metadata_row("Last update", last_update),
        metadata_row("Failures", status.failed_count.to_string()),
    ];
    if let Some(reason) = &status.reason {
        rows = rows.push(metadata_row("Reason", reason.clone()));
    }
    rows.spacing(4).into()
}

fn root_action_row(
    root: PathBuf,
    is_busy: bool,
    index_exists: bool,
    has_failures: bool,
) -> Element<'static, Message> {
    row![
        action_button(
            "Update",
            (!is_busy).then_some(Message::SearchIndexManualBuildRequested(
                root.clone(),
                FileSearchIndexMode::Incremental,
            )),
        ),
        action_button(
            "Rebuild",
            (!is_busy).then_some(Message::SearchIndexManualBuildRequested(
                root.clone(),
                FileSearchIndexMode::FullRebuild,
            )),
        ),
        action_button(
            "Delete index",
            (!is_busy && index_exists).then_some(Message::SearchIndexRemoveRequested(root.clone())),
        ),
        action_button(
            "Clear failures",
            (!is_busy && has_failures).then_some(Message::SearchIndexFailuresClearRequested(root)),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn root_action_row_without_status(root: PathBuf, is_busy: bool) -> Element<'static, Message> {
    row![
        action_button(
            "Update",
            (!is_busy).then_some(Message::SearchIndexManualBuildRequested(
                root.clone(),
                FileSearchIndexMode::Incremental,
            )),
        ),
        action_button(
            "Rebuild",
            (!is_busy).then_some(Message::SearchIndexManualBuildRequested(
                root,
                FileSearchIndexMode::FullRebuild,
            )),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn failures_panel(status: &FileSearchIndexStatus) -> Element<'static, Message> {
    if status.failures.is_empty() {
        return Space::new().height(Length::Fixed(0.0)).into();
    }

    let mut failures: Column<'static, Message> = Column::new()
        .spacing(6)
        .push(readable_text("Failures").size(13));
    for failure in &status.failures {
        failures = failures.push(failure_row(failure));
    }

    failures.into()
}

fn failure_row(failure: &FileSearchIndexFailure) -> Element<'static, Message> {
    let path = failure.path.to_string_lossy();
    let path = format_middle_ellipsized_text(path.as_ref(), FAILURE_PATH_MAX_CHARS);
    column![
        readable_text(path).size(12),
        readable_text(format!(
            "{} - last failed {}",
            failure.message,
            format_unix_ms(failure.last_failed_at_ms)
        ))
        .size(12),
    ]
    .spacing(2)
    .into()
}

fn metadata_row(label: &'static str, value: String) -> Element<'static, Message> {
    row![
        readable_text(label).size(12).width(Length::Fixed(96.0)),
        readable_text(value).size(12).width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn action_button(label: &'static str, message: Option<Message>) -> Button<'static, Message> {
    let button = button(container(readable_text(label).size(12)).padding([5, 8]))
        .style(context_menu_button_style());
    if let Some(message) = message {
        button.on_press(message)
    } else {
        button
    }
}

fn section_panel(content: Column<'_, Message>) -> Element<'_, Message> {
    container(content.spacing(8))
        .padding(10)
        .width(Length::Fill)
        .style(path_suggestion_item_style)
        .into()
}

fn format_unix_ms(ms: i64) -> String {
    let duration = Duration::from_millis(ms.unsigned_abs());
    let time = if ms >= 0 {
        UNIX_EPOCH.checked_add(duration)
    } else {
        UNIX_EPOCH.checked_sub(duration)
    };
    time.map(format_system_time)
        .unwrap_or_else(|| "Out of range".to_owned())
}

fn scoped_text<'a>(content: &'a str) -> Text<'a, Theme, iced::Renderer> {
    text(content)
}
