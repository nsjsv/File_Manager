use std::fmt;

use desktop_linux::{TerminalEmulator, TERMINAL_EMULATOR_OPTIONS};
use iced::widget::{button, column, container, pick_list, row, text_input, Button, Column, Space};
use iced::{Alignment, Element, Length};

use crate::app::FileBrowser;
use crate::appearance::context_menu_button_style;
use crate::model::{Message, ScrollbarRegion, ScrollbarVisibility, SettingsCategory};
use crate::typography::{localized_text, readable_text};

use super::application_logs::application_logs_settings_detail;
use super::auxiliary_window_layout::{
    auxiliary_detail_scroller, auxiliary_sidebar, auxiliary_sidebar_button, auxiliary_split_window,
};
use super::file_operation_verification_settings::file_operation_verification_options;
use super::network_settings::network_settings_content;
use super::option_controls::{
    action_choice_button, destructive_confirmation_button_style, selectable_choice_row,
};
use super::rendering_settings::rendering_gpu_preference_button;
use super::shortcut_settings::shortcut_settings_section;
use super::toggle_switch::switch_control;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TerminalEmulatorPickOption(TerminalEmulator);

impl fmt::Display for TerminalEmulatorPickOption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&crate::localization::translate_current(self.0.label()))
    }
}

pub(crate) fn view_settings_window(browser: &FileBrowser) -> Element<'_, Message> {
    let categories = settings_category_sidebar(browser.selected_settings_category);
    let detail = settings_category_detail(browser);

    auxiliary_split_window(categories, detail)
}

fn settings_category_sidebar(selected: SettingsCategory) -> Element<'static, Message> {
    let mut categories = Column::new()
        .spacing(6)
        .push(readable_text("Settings").size(18))
        .push(Space::new().height(Length::Fixed(6.0)));

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
        SettingsCategory::Logs => application_logs_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Network => network_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::FileOperations => {
            file_operation_settings_detail(browser, scrollbar_visibility)
        }
        SettingsCategory::Search => search_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Rendering => rendering_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Shortcuts => shortcut_settings_detail(browser, scrollbar_visibility),
    }
}

fn general_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    let show_custom_directory = browser.user_config().startup_location_policy
        == crate::config::StartupLocationPolicy::CustomDirectory;
    let content = if show_custom_directory {
        column![
            readable_text("General").size(20),
            readable_text("Language").size(13),
            language_setting_options(browser),
            readable_text("File display").size(13),
            hidden_files_visibility_button(browser),
            list_directory_size_display_mode_button(browser),
            readable_text("Startup").size(13),
            startup_location_options(browser),
            startup_custom_directory_input(browser),
            readable_text("Terminal").size(13),
            terminal_emulator_options(browser.terminal_emulator),
        ]
    } else {
        column![
            readable_text("General").size(20),
            readable_text("Language").size(13),
            language_setting_options(browser),
            readable_text("File display").size(13),
            hidden_files_visibility_button(browser),
            list_directory_size_display_mode_button(browser),
            readable_text("Startup").size(13),
            startup_location_options(browser),
            readable_text("Terminal").size(13),
            terminal_emulator_options(browser.terminal_emulator),
        ]
    }
    .spacing(10)
    .width(Length::Fill);

    settings_detail_scroller(content, scrollbar_visibility)
}

fn network_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(network_settings_content(browser), scrollbar_visibility)
}

fn file_operation_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            readable_text("File Operations").size(20),
            readable_text("Verification").size(13),
            file_operation_verification_options(browser.file_operation_verification()),
        ]
        .spacing(10)
        .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn rendering_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            readable_text("Rendering").size(20),
            rendering_gpu_preference_button(browser.rendering_gpu_preference),
        ]
        .spacing(10)
        .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn search_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            readable_text("Search").size(20),
            search_content_indexing_button(browser),
            localized_text(format!(
                "Maximum content extraction: {}",
                crate::formatting::format_file_size(browser.user_config().search_max_extract_bytes)
            ))
            .size(12),
            readable_text("Service and Index").size(13),
            search_service_status_rows(browser),
            search_service_recovery_controls(browser),
        ]
        .spacing(10)
        .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn search_service_recovery_controls(browser: &FileBrowser) -> Element<'static, Message> {
    use crate::model::search::SearchServiceRecoveryState;

    let recovery_is_running = browser.search.recovery.is_running();
    let restart_button = action_choice_button(
        "Restart Index Service",
        "Gracefully stop and restart the managed service.",
    );
    let restart_button = if recovery_is_running {
        restart_button
    } else {
        restart_button.on_press(Message::SearchServiceRestartRequested)
    };

    let force_restart_button =
        if browser.search.recovery == SearchServiceRecoveryState::ConfirmingForceRestart {
            action_choice_button(
                "Click Again to Force Restart",
                "Current indexing work will stop. Index data and settings will be kept.",
            )
            .style(destructive_confirmation_button_style())
        } else {
            action_choice_button(
                "Force Restart",
                "Use only when the managed service is unresponsive.",
            )
        };
    let force_restart_button = if recovery_is_running {
        force_restart_button
    } else {
        force_restart_button.on_press(Message::SearchServiceForceRestartPressed)
    };

    let mut controls = column![restart_button, force_restart_button]
        .spacing(6)
        .width(Length::Fill);
    if let Some(status_text) = search_service_recovery_status_text(&browser.search.recovery) {
        controls = controls.push(localized_text(status_text).size(12).width(Length::Fill));
    }
    controls.into()
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
        return container(localized_text(endpoint_message).size(12))
            .padding([5, 8])
            .width(Length::Fill)
            .into();
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

    container(status_content)
        .padding([5, 8])
        .width(Length::Fill)
        .into()
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

fn search_content_indexing_button(browser: &FileBrowser) -> Button<'static, Message> {
    let label = row![
        readable_text("Index File Contents")
            .size(12)
            .width(Length::Fill),
        switch_control(browser.user_config().search_content_indexing_enabled),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    button(container(label).padding([5, 8]).width(Length::Fill))
        .on_press(Message::SearchContentIndexingToggled)
        .width(Length::Fill)
        .style(context_menu_button_style())
}

fn shortcut_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            readable_text("Shortcuts").size(20),
            shortcut_settings_section(browser),
        ]
        .spacing(10)
        .width(Length::Fill),
        scrollbar_visibility,
    )
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

fn terminal_emulator_options(selected: TerminalEmulator) -> Element<'static, Message> {
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
    .width(Length::Fill)
    .text_size(12)
    .padding([5, 8])
    .into()
}

fn hidden_files_visibility_button(browser: &FileBrowser) -> Button<'static, Message> {
    let label = row![
        readable_text("Show Hidden Files")
            .size(12)
            .width(Length::Fill),
        switch_control(browser.options.include_hidden),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    button(container(label).padding([5, 8]).width(Length::Fill))
        .on_press(Message::ShowHiddenFilesToggled)
        .width(Length::Fill)
        .style(context_menu_button_style())
}

fn list_directory_size_display_mode_button(browser: &FileBrowser) -> Button<'static, Message> {
    let uses_recursive_total_size = browser
        .user_config()
        .list_directory_size_display_mode
        .uses_recursive_total_size();
    let label = row![
        readable_text("Show Recursive Folder Size In List View")
            .size(12)
            .width(Length::Fill),
        switch_control(uses_recursive_total_size),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    button(container(label).padding([5, 8]).width(Length::Fill))
        .on_press(Message::ListDirectorySizeDisplayModeToggled)
        .width(Length::Fill)
        .style(context_menu_button_style())
}

fn startup_location_options(browser: &FileBrowser) -> Element<'_, Message> {
    let policy = browser.user_config().startup_location_policy;
    column![
        selectable_choice_row(
            "Home directory",
            "Open your home directory on startup.",
            policy == crate::config::StartupLocationPolicy::Home,
            Message::StartupLocationPolicySelected(crate::config::StartupLocationPolicy::Home),
        ),
        selectable_choice_row(
            "Custom directory",
            "Open the configured directory on startup.",
            policy == crate::config::StartupLocationPolicy::CustomDirectory,
            Message::StartupLocationPolicySelected(
                crate::config::StartupLocationPolicy::CustomDirectory,
            ),
        ),
        selectable_choice_row(
            "Previous state",
            "Start in the state from the last close, preserving views and directories.",
            policy == crate::config::StartupLocationPolicy::PreviousSession,
            Message::StartupLocationPolicySelected(
                crate::config::StartupLocationPolicy::PreviousSession,
            ),
        ),
    ]
    .spacing(6)
    .into()
}

fn language_setting_options(browser: &FileBrowser) -> Element<'_, Message> {
    let setting = browser.user_config().language_setting;
    column![
        selectable_choice_row(
            "Auto",
            "Use the detected system language.",
            setting == crate::config::UiLanguageSetting::System,
            Message::LanguageSettingSelected(crate::config::UiLanguageSetting::System),
        ),
        selectable_choice_row(
            "English",
            "Always show the interface in English.",
            setting == crate::config::UiLanguageSetting::English,
            Message::LanguageSettingSelected(crate::config::UiLanguageSetting::English),
        ),
        selectable_choice_row(
            "中文",
            "Always show the interface in Chinese.",
            setting == crate::config::UiLanguageSetting::Chinese,
            Message::LanguageSettingSelected(crate::config::UiLanguageSetting::Chinese),
        ),
    ]
    .spacing(6)
    .into()
}

fn startup_custom_directory_input(browser: &FileBrowser) -> Element<'_, Message> {
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
    container(content)
        .padding([5, 8])
        .width(Length::Fill)
        .into()
}
