mod errors;
mod overview;
mod path_rules;

use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use file_index::{DirectoryErrorPolicy, MediaMetadataScope};
use iced::widget::{button, column, container, row, Button, Column};
use iced::{Alignment, Element, Length};

use crate::app::FileBrowser;
use crate::appearance::context_menu_button_style;
use crate::config::SearchBackendMode;
use crate::formatting::{format_middle_ellipsized_text, format_system_time};
use crate::model::{Message, SearchIndexDaemonStatus, SearchIndexSettingsSection};
use crate::typography::readable_text;

use super::option_controls::selectable_choice_row;

pub(super) const ROOT_PATH_MAX_CHARS: usize = 72;
pub(super) const INDEX_DIR_MAX_CHARS: usize = 76;
pub(super) const FAILURE_PATH_MAX_CHARS: usize = 78;
pub(super) const PATH_RULE_MAX_CHARS: usize = 84;

pub(super) fn search_index_settings_content(browser: &FileBrowser) -> Column<'_, Message> {
    let content = match browser.search_backend_mode() {
        SearchBackendMode::Simple => column![settings_header(), search_mode_panel(browser)],
        SearchBackendMode::Indexed => match browser.search_index.selected_settings_section {
            SearchIndexSettingsSection::Overview => column![
                settings_header(),
                search_mode_panel(browser),
                profile_policy_panel(browser),
                index_directory_panel(browser),
                overview::search_index_overview_content(browser),
            ],
            SearchIndexSettingsSection::Errors => column![
                settings_header(),
                errors::search_index_errors_content(browser),
            ],
            SearchIndexSettingsSection::PathRules => column![
                settings_header(),
                path_rules::search_index_path_rules_content(browser),
            ],
        },
    };

    content.spacing(12).width(Length::Fill)
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
            "Uses configured indexed paths and optional media metadata indexing.",
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
        crate::localization::translate_current("No roots selected")
    } else {
        format!(
            "{} explicit path(s)",
            browser.search_index.profile_roots.len()
        )
    };
    let daemon_state = search_index_daemon_status_label(
        browser.search_index.daemon_status.as_ref(),
        browser.search_index.daemon_status_loading,
        browser.search_index.bootstrap_in_progress || browser.search_index.profile_loading,
    );
    let daemon_controls_enabled = !browser.search_index.daemon_status_loading;
    let mut profile = column![
        readable_text("Profile").size(13),
        metadata_row("Service", daemon_state),
        metadata_row("Profile id", browser.search_index.profile_id.clone()),
        metadata_row("Roots", roots_label),
        row![
            action_button(
                "Restart service",
                daemon_controls_enabled.then_some(Message::SearchIndexDaemonRestartRequested),
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

    profile =
        profile.push(readable_text("Changing media indexing applies to future rebuilds.").size(12));

    section_panel(profile)
}

fn search_index_daemon_status_label(
    status: Option<&SearchIndexDaemonStatus>,
    loading: bool,
    preparing: bool,
) -> String {
    if loading {
        return crate::localization::translate_current("Checking...");
    }
    if preparing {
        return crate::localization::translate_current("Preparing...");
    }
    match status {
        Some(SearchIndexDaemonStatus::Reachable) => {
            crate::localization::translate_current("Connected")
        }
        Some(SearchIndexDaemonStatus::Unreachable(_)) => {
            crate::localization::translate_current("Unavailable")
        }
        None => crate::localization::translate_current("Unknown"),
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
                Message::SearchIndexDirectoryErrorPolicySelected(DirectoryErrorPolicy::Abort,)
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

pub(super) fn root_action_row(
    root: PathBuf,
    is_busy: bool,
    index_exists: bool,
    has_failures: bool,
) -> Element<'static, Message> {
    row![
        action_button(
            "Rebuild",
            (!is_busy).then_some(Message::SearchIndexManualBuildRequested(root.clone())),
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

pub(super) fn root_action_row_without_status(
    root: PathBuf,
    is_busy: bool,
) -> Element<'static, Message> {
    row![action_button(
        "Rebuild",
        (!is_busy).then_some(Message::SearchIndexManualBuildRequested(root)),
    ),]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

pub(super) fn metadata_row(label: &'static str, value: String) -> Element<'static, Message> {
    row![
        readable_text(label).size(12).width(Length::Fixed(96.0)),
        readable_text(value).size(12).width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

pub(super) fn action_button(
    label: &'static str,
    message: Option<Message>,
) -> Button<'static, Message> {
    let button = button(container(readable_text(label).size(12)).padding([5, 8]))
        .style(context_menu_button_style());
    if let Some(message) = message {
        button.on_press(message)
    } else {
        button
    }
}

pub(super) fn section_panel(content: Column<'_, Message>) -> Element<'_, Message> {
    container(content.spacing(8))
        .padding(10)
        .width(Length::Fill)
        .style(crate::appearance::path_suggestion_item_style)
        .into()
}

pub(super) fn format_unix_ms(ms: i64) -> String {
    let duration = Duration::from_millis(ms.unsigned_abs());
    let time = if ms >= 0 {
        UNIX_EPOCH.checked_add(duration)
    } else {
        UNIX_EPOCH.checked_sub(duration)
    };
    time.map(format_system_time)
        .unwrap_or_else(|| "Out of range".to_owned())
}
