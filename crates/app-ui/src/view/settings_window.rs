use std::fmt;

use desktop_linux::{TerminalEmulator, TERMINAL_EMULATOR_OPTIONS};
use iced::widget::{button, column, container, pick_list, row, text_input, Button, Column};
use iced::{Alignment, Element, Length};

use crate::app::FileBrowser;
use crate::appearance::context_menu_button_style;
use crate::config::{StartupLocationPolicy, UiLanguageSetting};
use crate::model::{Message, ScrollbarRegion, ScrollbarVisibility, SettingsCategory};
use crate::typography::{localized_text, readable_text};

use super::application_logs::application_logs_settings_detail;
use super::auxiliary_window_layout::{
    auxiliary_detail_scroller, auxiliary_sidebar, auxiliary_sidebar_button, auxiliary_split_window,
};
use super::file_operation_verification_settings::file_operation_verification_options;
use super::network_settings::{max_preview_file_size_row, network_thumbnails_row};
use super::option_controls::destructive_confirmation_button_style;
use super::rendering_settings::rendering_gpu_preference_row;
use super::settings_group::{
    action_setting_row, info_setting_row, labeled_setting_row, settings_card, settings_group,
    toggle_setting_row, SETTINGS_GROUP_SPACING,
};
use super::shortcut_settings::shortcut_settings_section;
use super::window_control_settings::window_control_settings_row;

const SETTINGS_DROPDOWN_WIDTH: f32 = 220.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TerminalEmulatorPickOption(TerminalEmulator);

impl fmt::Display for TerminalEmulatorPickOption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&crate::localization::translate_current(self.0.label()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LanguageSettingPickOption(UiLanguageSetting);

impl fmt::Display for LanguageSettingPickOption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&crate::localization::translate_current(
            language_setting_label(self.0),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartupLocationPickOption(StartupLocationPolicy);

impl fmt::Display for StartupLocationPickOption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&crate::localization::translate_current(
            startup_location_label(self.0),
        ))
    }
}

pub(crate) fn view_settings_window(browser: &FileBrowser) -> Element<'_, Message> {
    let categories = settings_category_sidebar(browser.selected_settings_category);
    let detail = settings_category_detail(browser);

    auxiliary_split_window(categories, detail)
}

fn settings_category_sidebar(selected: SettingsCategory) -> Element<'static, Message> {
    let mut categories = Column::new().spacing(4);
    for category in SettingsCategory::ALL {
        categories = categories.push(settings_category_button(category, selected));
    }

    auxiliary_sidebar(categories)
}

fn settings_category_button(
    category: SettingsCategory,
    selected: SettingsCategory,
) -> Button<'static, Message> {
    auxiliary_sidebar_button(
        category.label(),
        category == selected,
        Message::SettingsCategorySelected(category),
    )
}

fn settings_category_detail(browser: &FileBrowser) -> Element<'_, Message> {
    let scrollbar_visibility = browser.scrollbar_visibility_for(&ScrollbarRegion::Settings);
    match browser.selected_settings_category {
        SettingsCategory::General => general_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Appearance => appearance_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Files => files_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Search => search_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Shortcuts => shortcut_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Logs => application_logs_settings_detail(browser, scrollbar_visibility),
    }
}

fn general_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    let mut rows = vec![
        labeled_setting_row("Language", language_setting_dropdown(browser)),
        labeled_setting_row("Startup location", startup_location_dropdown(browser)),
    ];
    if browser.user_config().startup_location_policy == StartupLocationPolicy::CustomDirectory {
        rows.push(startup_custom_directory_row(browser));
    }
    rows.push(labeled_setting_row(
        "Terminal",
        terminal_emulator_dropdown(browser.terminal_emulator),
    ));

    settings_detail_scroller(
        column![settings_card(rows)]
            .spacing(SETTINGS_GROUP_SPACING)
            .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn appearance_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            settings_group(
                "Window controls",
                vec![window_control_settings_row(browser)]
            ),
            settings_group(
                "Rendering",
                vec![rendering_gpu_preference_row(
                    browser.rendering_gpu_preference
                )],
            ),
        ]
        .spacing(SETTINGS_GROUP_SPACING)
        .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn files_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            settings_group(
                "File display",
                vec![
                    toggle_setting_row(
                        "Show Hidden Files",
                        None,
                        browser.options.include_hidden,
                        Message::ShowHiddenFilesToggled,
                    ),
                    toggle_setting_row(
                        "Show Recursive Folder Size In List View",
                        None,
                        browser
                            .user_config()
                            .list_directory_size_display_mode
                            .uses_recursive_total_size(),
                        Message::ListDirectorySizeDisplayModeToggled,
                    ),
                ],
            ),
            settings_group(
                "Verification",
                vec![file_operation_verification_options(
                    browser.file_operation_verification(),
                )],
            ),
            settings_group(
                "Network",
                vec![
                    network_thumbnails_row(browser),
                    max_preview_file_size_row(browser),
                ],
            ),
        ]
        .spacing(SETTINGS_GROUP_SPACING)
        .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn search_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    let mut service_rows = vec![search_service_status_rows(browser)];
    service_rows.extend(search_service_recovery_rows(browser));

    settings_detail_scroller(
        column![
            settings_card(vec![
                toggle_setting_row(
                    "Index File Contents",
                    None,
                    browser.user_config().search_content_indexing_enabled,
                    Message::SearchContentIndexingToggled,
                ),
                info_setting_row(
                    localized_text(format!(
                        "Maximum content extraction: {}",
                        crate::formatting::format_file_size(
                            browser.user_config().search_max_extract_bytes
                        )
                    ))
                    .size(12)
                    .into(),
                ),
            ]),
            settings_group("Service and Index", service_rows),
        ]
        .spacing(SETTINGS_GROUP_SPACING)
        .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn shortcut_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![shortcut_settings_section(browser)]
            .spacing(SETTINGS_GROUP_SPACING)
            .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn search_service_recovery_rows(browser: &FileBrowser) -> Vec<Element<'static, Message>> {
    use crate::model::search::SearchServiceRecoveryState;

    let recovery_is_running = browser.search.recovery.is_running();
    let restart_row = action_setting_row(
        "Restart Index Service",
        "Gracefully stop and restart the managed service.",
    );
    let restart_row = if recovery_is_running {
        restart_row
    } else {
        restart_row.on_press(Message::SearchServiceRestartRequested)
    };

    let force_restart_row =
        if browser.search.recovery == SearchServiceRecoveryState::ConfirmingForceRestart {
            action_setting_row(
                "Click Again to Force Restart",
                "Current indexing work will stop. Index data and settings will be kept.",
            )
            .style(destructive_confirmation_button_style())
        } else {
            action_setting_row(
                "Force Restart",
                "Use only when the managed service is unresponsive.",
            )
        };
    let force_restart_row = if recovery_is_running {
        force_restart_row
    } else {
        force_restart_row.on_press(Message::SearchServiceForceRestartPressed)
    };

    let mut rows: Vec<Element<'static, Message>> =
        vec![restart_row.into(), force_restart_row.into()];
    if let Some(status_text) = search_service_recovery_status_text(&browser.search.recovery) {
        rows.push(info_setting_row(
            localized_text(status_text)
                .size(12)
                .width(Length::Fill)
                .into(),
        ));
    }
    rows
}

fn search_service_recovery_status_text(
    state: &crate::model::search::SearchServiceRecoveryState,
) -> Option<String> {
    use crate::model::search::{SearchServiceRecoveryAction, SearchServiceRecoveryState};

    match state {
        SearchServiceRecoveryState::Idle | SearchServiceRecoveryState::ConfirmingForceRestart => {
            None
        }
        SearchServiceRecoveryState::Running(SearchServiceRecoveryAction::Restart) => {
            Some("Restarting index service...".to_owned())
        }
        SearchServiceRecoveryState::Running(SearchServiceRecoveryAction::ForceRestart) => {
            Some("Force restarting index service...".to_owned())
        }
        SearchServiceRecoveryState::Succeeded(SearchServiceRecoveryAction::Restart) => {
            Some("Index service restarted successfully.".to_owned())
        }
        SearchServiceRecoveryState::Succeeded(SearchServiceRecoveryAction::ForceRestart) => {
            Some("Index service force restarted successfully.".to_owned())
        }
        SearchServiceRecoveryState::Failed {
            action: SearchServiceRecoveryAction::Restart,
            message,
        } => Some(format!("Could not restart index service: {message}")),
        SearchServiceRecoveryState::Failed {
            action: SearchServiceRecoveryAction::ForceRestart,
            message,
        } => Some(format!("Could not force restart index service: {message}")),
    }
}

#[cfg(test)]
mod search_service_recovery_presentation_tests {
    use crate::model::search::{SearchServiceRecoveryAction, SearchServiceRecoveryState};

    use super::search_service_recovery_status_text;

    #[test]
    fn recovery_status_distinguishes_action_phase_and_failure() {
        assert_eq!(
            search_service_recovery_status_text(&SearchServiceRecoveryState::Running(
                SearchServiceRecoveryAction::Restart
            ))
            .as_deref(),
            Some("Restarting index service...")
        );
        assert_eq!(
            search_service_recovery_status_text(&SearchServiceRecoveryState::Succeeded(
                SearchServiceRecoveryAction::ForceRestart
            ))
            .as_deref(),
            Some("Index service force restarted successfully.")
        );
        assert_eq!(
            search_service_recovery_status_text(&SearchServiceRecoveryState::Failed {
                action: SearchServiceRecoveryAction::Restart,
                message: "permission denied".to_owned(),
            })
            .as_deref(),
            Some("Could not restart index service: permission denied")
        );
        assert!(search_service_recovery_status_text(
            &SearchServiceRecoveryState::ConfirmingForceRestart
        )
        .is_none());
    }
}

fn search_service_status_rows(browser: &FileBrowser) -> Element<'_, Message> {
    use file_search::{IndexedQueryAvailability, SearchServicePhase};

    use crate::model::search::SearchEndpointState;

    let SearchEndpointState::Connected(status) = &browser.search.endpoint else {
        let endpoint_message = match &browser.search.endpoint {
            SearchEndpointState::Starting => "Search endpoint is starting…".to_owned(),
            SearchEndpointState::Unavailable { message } => {
                format!("Search endpoint unavailable: {message}")
            }
            SearchEndpointState::Connected(_) => unreachable!(),
        };
        return info_setting_row(localized_text(endpoint_message).size(12).into());
    };

    let phase_message = match &status.phase {
        SearchServicePhase::Starting => "Service: starting".to_owned(),
        SearchServicePhase::Ready => "Service: ready".to_owned(),
        SearchServicePhase::Degraded { message } => format!("Service degraded: {message}"),
        SearchServicePhase::Failed { message } => format!("Service failed: {message}"),
        SearchServicePhase::ShuttingDown => "Service: shutting down".to_owned(),
    };
    let query_message = match &status.query_availability {
        IndexedQueryAvailability::Available => "Indexed queries: available".to_owned(),
        IndexedQueryAvailability::Unavailable { message } => {
            format!("Indexed queries unavailable: {message}")
        }
    };
    let mut status_content = column![
        readable_text("Search endpoint connected").size(12),
        localized_text(phase_message).size(12),
        localized_text(query_message).size(12),
    ]
    .spacing(3);
    let index_status_lines = status
        .index_status
        .as_ref()
        .map(index_status_lines)
        .unwrap_or_else(|| vec!["Index status: Not initialized".to_owned()]);
    for (line_index, line) in index_status_lines.into_iter().enumerate() {
        status_content =
            status_content.push(localized_text(line).size(if line_index == 0 { 13 } else { 12 }));
    }

    info_setting_row(status_content.into())
}

fn index_status_lines(status: &file_search::IndexStatus) -> Vec<String> {
    use file_search::{IndexHealth, IndexPhase};

    let mut lines = match &status.phase {
        IndexPhase::Starting => vec!["Index status: Starting".to_owned()],
        IndexPhase::Checking {
            checked_entries,
            changed_entries,
        } => vec![
            "Index status: Checking".to_owned(),
            format!("Checked: {} items", format_count(*checked_entries)),
            format!("Changed: {} items", format_count(*changed_entries)),
        ],
        IndexPhase::Crawling {
            scanned_entries,
            current_scope,
        } => vec![
            "Index status: Crawling".to_owned(),
            format!("Scanned: {} items", format_count(*scanned_entries)),
            format!("Scope: {}", current_scope.to_string_lossy()),
        ],
        IndexPhase::Applying { pending_mutations } => vec![
            "Index status: Applying".to_owned(),
            format!("Pending changes: {}", format_count(*pending_mutations)),
        ],
        IndexPhase::Complete => vec!["Index status: Complete".to_owned()],
        IndexPhase::Failed { message } => vec![
            "Index status: Failed".to_owned(),
            format!("Index error: {message}"),
        ],
    };
    lines.insert(
        1,
        format!(
            "Indexed: {} items",
            format_count(status.visible_indexed_files)
        ),
    );
    lines.push(match &status.health {
        IndexHealth::Healthy => "Index maintenance: Healthy".to_owned(),
        IndexHealth::Degraded { message } => {
            format!("Index maintenance: Degraded ({message})")
        }
        IndexHealth::Error { message } => format!("Index maintenance: Error ({message})"),
    });
    lines
}

#[cfg(test)]
mod index_status_presentation_tests {
    use file_search::{IndexHealth, IndexPhase, IndexStatus};

    use super::index_status_lines;

    fn status(phase: IndexPhase, health: IndexHealth) -> IndexStatus {
        IndexStatus {
            phase,
            visible_indexed_files: 98_765,
            health,
            capabilities: Vec::new(),
        }
    }

    #[test]
    fn index_phase_lines_distinguish_starting_checking_crawling_applying_complete_and_failed() {
        assert_eq!(
            index_status_lines(&status(IndexPhase::Starting, IndexHealth::Healthy)),
            vec![
                "Index status: Starting".to_owned(),
                "Indexed: 98,765 items".to_owned(),
                "Index maintenance: Healthy".to_owned(),
            ]
        );
        assert_eq!(
            index_status_lines(&status(
                IndexPhase::Checking {
                    checked_entries: 12_345,
                    changed_entries: 6,
                },
                IndexHealth::Healthy,
            )),
            vec![
                "Index status: Checking".to_owned(),
                "Indexed: 98,765 items".to_owned(),
                "Checked: 12,345 items".to_owned(),
                "Changed: 6 items".to_owned(),
                "Index maintenance: Healthy".to_owned(),
            ]
        );
        assert_eq!(
            index_status_lines(&status(
                IndexPhase::Crawling {
                    scanned_entries: 321,
                    current_scope: "/home/me/new".into(),
                },
                IndexHealth::Healthy,
            )),
            vec![
                "Index status: Crawling".to_owned(),
                "Indexed: 98,765 items".to_owned(),
                "Scanned: 321 items".to_owned(),
                "Scope: /home/me/new".to_owned(),
                "Index maintenance: Healthy".to_owned(),
            ]
        );
        assert_eq!(
            index_status_lines(&status(
                IndexPhase::Applying {
                    pending_mutations: 17,
                },
                IndexHealth::Healthy,
            )),
            vec![
                "Index status: Applying".to_owned(),
                "Indexed: 98,765 items".to_owned(),
                "Pending changes: 17".to_owned(),
                "Index maintenance: Healthy".to_owned(),
            ]
        );
        assert_eq!(
            index_status_lines(&status(IndexPhase::Complete, IndexHealth::Healthy)),
            vec![
                "Index status: Complete".to_owned(),
                "Indexed: 98,765 items".to_owned(),
                "Index maintenance: Healthy".to_owned(),
            ]
        );
        assert_eq!(
            index_status_lines(&status(
                IndexPhase::Failed {
                    message: "database unavailable".to_owned(),
                },
                IndexHealth::Healthy,
            )),
            vec![
                "Index status: Failed".to_owned(),
                "Indexed: 98,765 items".to_owned(),
                "Index error: database unavailable".to_owned(),
                "Index maintenance: Healthy".to_owned(),
            ]
        );
    }

    #[test]
    fn stable_count_remains_visible_when_maintenance_is_degraded() {
        assert_eq!(
            index_status_lines(&status(
                IndexPhase::Complete,
                IndexHealth::Degraded {
                    message: "one directory is not watched".to_owned(),
                },
            )),
            vec![
                "Index status: Complete".to_owned(),
                "Indexed: 98,765 items".to_owned(),
                "Index maintenance: Degraded (one directory is not watched)".to_owned(),
            ]
        );
    }

    #[test]
    fn active_progress_remains_visible_when_maintenance_has_failed() {
        assert_eq!(
            index_status_lines(&status(
                IndexPhase::Checking {
                    checked_entries: 128,
                    changed_entries: 2,
                },
                IndexHealth::Error {
                    message: "watcher stopped".to_owned(),
                },
            )),
            vec![
                "Index status: Checking".to_owned(),
                "Indexed: 98,765 items".to_owned(),
                "Checked: 128 items".to_owned(),
                "Changed: 2 items".to_owned(),
                "Index maintenance: Error (watcher stopped)".to_owned(),
            ]
        );
    }
}

fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let bytes = digits.as_bytes();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 && (bytes.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(*byte as char);
    }
    grouped
}

fn settings_detail_scroller<'a>(
    content: Column<'a, Message>,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'a, Message> {
    auxiliary_detail_scroller(
        content,
        ScrollbarRegion::Settings,
        scrollbar_visibility,
        Message::SettingsScrolled,
    )
}

fn language_setting_label(setting: UiLanguageSetting) -> &'static str {
    match setting {
        UiLanguageSetting::System => "Auto",
        UiLanguageSetting::English => "English",
        UiLanguageSetting::Chinese => "中文",
    }
}

fn startup_location_label(policy: StartupLocationPolicy) -> &'static str {
    match policy {
        StartupLocationPolicy::Home => "Home directory",
        StartupLocationPolicy::CustomDirectory => "Custom directory",
        StartupLocationPolicy::PreviousSession => "Previous state",
    }
}

fn language_setting_dropdown(browser: &FileBrowser) -> Element<'static, Message> {
    let options = [
        UiLanguageSetting::System,
        UiLanguageSetting::English,
        UiLanguageSetting::Chinese,
    ]
    .into_iter()
    .map(LanguageSettingPickOption)
    .collect::<Vec<_>>();

    pick_list(
        options,
        Some(LanguageSettingPickOption(
            browser.user_config().language_setting,
        )),
        |selected| Message::LanguageSettingSelected(selected.0),
    )
    .width(Length::Fixed(SETTINGS_DROPDOWN_WIDTH))
    .text_size(12)
    .padding([5, 8])
    .into()
}

fn startup_location_dropdown(browser: &FileBrowser) -> Element<'static, Message> {
    let options = [
        StartupLocationPolicy::Home,
        StartupLocationPolicy::CustomDirectory,
        StartupLocationPolicy::PreviousSession,
    ]
    .into_iter()
    .map(StartupLocationPickOption)
    .collect::<Vec<_>>();

    pick_list(
        options,
        Some(StartupLocationPickOption(
            browser.user_config().startup_location_policy,
        )),
        |selected| Message::StartupLocationPolicySelected(selected.0),
    )
    .width(Length::Fixed(SETTINGS_DROPDOWN_WIDTH))
    .text_size(12)
    .padding([5, 8])
    .into()
}

fn terminal_emulator_dropdown(selected: TerminalEmulator) -> Element<'static, Message> {
    let options = TERMINAL_EMULATOR_OPTIONS
        .iter()
        .copied()
        .map(TerminalEmulatorPickOption)
        .collect::<Vec<_>>();

    pick_list(
        options,
        Some(TerminalEmulatorPickOption(selected)),
        |selected| Message::TerminalEmulatorSelected(selected.0),
    )
    .width(Length::Fixed(SETTINGS_DROPDOWN_WIDTH))
    .text_size(12)
    .padding([5, 8])
    .into()
}

fn startup_custom_directory_row(browser: &FileBrowser) -> Element<'_, Message> {
    let input = text_input(
        &crate::localization::translate_current("Directory"),
        &browser.startup_custom_directory_input,
    )
    .on_input(Message::StartupCustomDirectoryInputChanged)
    .on_submit(Message::StartupCustomDirectoryCommitted)
    .padding([6, 8])
    .size(12)
    .width(Length::Fill);
    let save = button(container(readable_text("Save").size(12)).padding([6, 10]))
        .on_press(Message::StartupCustomDirectoryCommitted)
        .style(context_menu_button_style());
    let mut content = column![row![
        readable_text("Custom Startup Directory")
            .size(12)
            .width(Length::FillPortion(2)),
        input,
        save,
    ]
    .spacing(8)
    .align_y(Alignment::Center)]
    .spacing(3);
    if let Some(error) = &browser.startup_custom_directory_error {
        content = content.push(localized_text(error).size(11).width(Length::Fill));
    }
    info_setting_row(content.into())
}
