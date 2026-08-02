use file_search::{
    IndexHealth, IndexPhase, IndexedQueryAvailability, SearchServicePhase, SearchServiceStatus,
};
use iced::widget::{button, column, container, Column};
use iced::{Element, Length};

use crate::app::FileBrowser;
use crate::appearance::context_menu_button_style;
use crate::formatting::{format_file_size, format_system_time};
use crate::model::{
    Message, ScrollbarRegion, ScrollbarVisibility, SearchEndpointState,
    SearchServiceDiagnosticKind, SearchServiceIncident, SearchServiceIncidentState,
    SearchServiceRecoveryAction, SearchServiceRecoveryState,
};
use crate::typography::{localized_text, readable_text};

use super::auxiliary_window_layout::auxiliary_detail_scroller;
use super::option_controls::destructive_confirmation_button_style;
use super::settings_group::{
    action_setting_row, info_setting_row, labeled_setting_row, settings_group, toggle_setting_row,
    SETTINGS_GROUP_SPACING,
};

pub(super) fn search_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    let content = column![
        settings_group("Content indexing", content_indexing_rows(browser)),
        settings_group("Status overview", status_overview_rows(browser)),
        settings_group("Index progress", index_progress_rows(browser)),
        settings_group("Recent issues", recent_issue_rows(browser)),
        settings_group("Service recovery", service_recovery_rows(browser)),
    ]
    .spacing(SETTINGS_GROUP_SPACING)
    .width(Length::Fill);

    auxiliary_detail_scroller(
        content,
        ScrollbarRegion::Settings,
        scrollbar_visibility,
        Message::SettingsScrolled,
    )
}

fn content_indexing_rows(browser: &FileBrowser) -> Vec<Element<'_, Message>> {
    vec![
        toggle_setting_row(
            "Index File Contents",
            None,
            browser.user_config().search_content_indexing_enabled,
            Message::SearchContentIndexingToggled,
        ),
        info_setting_row(
            localized_text(format!(
                "Maximum content extraction: {}",
                format_file_size(browser.user_config().search_max_extract_bytes)
            ))
            .size(12)
            .into(),
        ),
    ]
}

fn status_overview_rows(browser: &FileBrowser) -> Vec<Element<'_, Message>> {
    let service = &browser.search_service;
    let mut rows = Vec::new();

    match &service.endpoint {
        SearchEndpointState::Starting => rows.push(status_notice(
            "Checking the search service. Status will update automatically.",
        )),
        SearchEndpointState::Connected => {}
        SearchEndpointState::Rechecking {
            consecutive_failures,
            ..
        } => rows.push(status_notice(format!(
            "Connection is unstable. Rechecking automatically ({consecutive_failures}/3); showing the last confirmed status."
        ))),
        SearchEndpointState::Unavailable { diagnostic } => {
            rows.push(diagnostic_notice(diagnostic.kind));
        }
    }

    if let Some(status) = service.confirmed_status.as_ref() {
        rows.extend(confirmed_status_summary_rows(status));
        if let Some(confirmed_at) = service.last_confirmed_at {
            rows.push(info_setting_row(
                localized_text(format!(
                    "Last confirmed: {}",
                    format_system_time(confirmed_at)
                ))
                .size(11)
                .into(),
            ));
        }
    } else {
        rows.extend([
            summary_value_row(
                "Service",
                endpoint_without_snapshot_label(&service.endpoint),
            ),
            summary_value_row("Indexed search", "Waiting for service status"),
            summary_value_row("Index maintenance", "Not initialized"),
        ]);
    }

    rows
}

fn confirmed_status_summary_rows(status: &SearchServiceStatus) -> Vec<Element<'static, Message>> {
    vec![
        summary_value_row("Service", service_phase_label(&status.phase)),
        summary_value_row(
            "Indexed search",
            query_availability_label(&status.query_availability),
        ),
        summary_value_row("Index maintenance", index_maintenance_label(status)),
    ]
}

fn service_phase_label(phase: &SearchServicePhase) -> &'static str {
    match phase {
        SearchServicePhase::Starting => "Starting",
        SearchServicePhase::Ready => "Running",
        SearchServicePhase::Degraded { .. } => "Needs attention",
        SearchServicePhase::Failed { .. } => "Unavailable",
        SearchServicePhase::ShuttingDown => "Stopping",
    }
}

fn query_availability_label(availability: &IndexedQueryAvailability) -> &'static str {
    match availability {
        IndexedQueryAvailability::Available => "Available",
        IndexedQueryAvailability::Unavailable { .. } => "Temporarily unavailable",
    }
}

fn index_maintenance_label(status: &SearchServiceStatus) -> &'static str {
    let Some(index_status) = status.index_status.as_ref() else {
        return "Not initialized";
    };
    match index_status.health {
        IndexHealth::Healthy => "Healthy",
        IndexHealth::Degraded { .. } => "Needs attention",
        IndexHealth::Error { .. } => "Unavailable",
    }
}

fn endpoint_without_snapshot_label(endpoint: &SearchEndpointState) -> &'static str {
    match endpoint {
        SearchEndpointState::Starting => "Checking",
        SearchEndpointState::Connected => "Running",
        SearchEndpointState::Rechecking { .. } => "Rechecking",
        SearchEndpointState::Unavailable { .. } => "Unavailable",
    }
}

fn diagnostic_notice(kind: SearchServiceDiagnosticKind) -> Element<'static, Message> {
    info_setting_row(
        column![
            localized_text(diagnostic_title(kind)).size(13),
            localized_text(diagnostic_recommendation(kind))
                .size(11)
                .width(Length::Fill),
        ]
        .spacing(3)
        .width(Length::Fill)
        .into(),
    )
}

fn status_notice(message: impl Into<String>) -> Element<'static, Message> {
    info_setting_row(
        localized_text(message.into())
            .size(11)
            .width(Length::Fill)
            .into(),
    )
}

fn summary_value_row(label: &'static str, value: impl Into<String>) -> Element<'static, Message> {
    labeled_setting_row(
        label,
        localized_text(value.into())
            .size(12)
            .width(Length::Shrink)
            .into(),
    )
}

fn index_progress_rows(browser: &FileBrowser) -> Vec<Element<'_, Message>> {
    let Some(status) = browser.search_service.confirmed_status.as_ref() else {
        return vec![status_notice(
            "Index progress will appear after the service responds.",
        )];
    };
    let Some(index_status) = status.index_status.as_ref() else {
        return vec![status_notice("The index has not been initialized yet.")];
    };

    let mut rows = vec![
        summary_value_row("Index phase", index_phase_label(&index_status.phase)),
        summary_value_row(
            "Indexed items",
            format_count(index_status.visible_indexed_files),
        ),
    ];
    match &index_status.phase {
        IndexPhase::Checking {
            checked_entries,
            changed_entries,
        } => {
            rows.push(summary_value_row(
                "Checked items",
                format_count(*checked_entries),
            ));
            rows.push(summary_value_row(
                "Changed items",
                format_count(*changed_entries),
            ));
        }
        IndexPhase::Crawling {
            scanned_entries,
            current_scope,
        } => {
            rows.push(summary_value_row(
                "Scanned items",
                format_count(*scanned_entries),
            ));
            rows.push(summary_value_row(
                "Current location",
                current_scope.to_string_lossy().into_owned(),
            ));
        }
        IndexPhase::Applying { pending_mutations } => rows.push(summary_value_row(
            "Pending changes",
            format_count(*pending_mutations),
        )),
        IndexPhase::Starting | IndexPhase::Complete | IndexPhase::Failed { .. } => {}
    }
    rows
}

fn index_phase_label(phase: &IndexPhase) -> &'static str {
    match phase {
        IndexPhase::Starting => "Starting",
        IndexPhase::Checking { .. } => "Checking existing files",
        IndexPhase::Crawling { .. } => "Scanning files",
        IndexPhase::Applying { .. } => "Applying changes",
        IndexPhase::Complete => "Up to date",
        IndexPhase::Failed { .. } => "Failed",
    }
}

fn recent_issue_rows(browser: &FileBrowser) -> Vec<Element<'_, Message>> {
    if browser.search_service.incidents.is_empty() {
        return vec![status_notice(
            "No search service issues detected during this app session.",
        )];
    }

    browser
        .search_service
        .incidents
        .iter()
        .map(issue_row)
        .collect()
}

fn issue_row(incident: &SearchServiceIncident) -> Element<'_, Message> {
    let state_label = match incident.state {
        SearchServiceIncidentState::Current => "Current issue",
        SearchServiceIncidentState::Recovered => "Recovered",
    };
    let occurrence_label = if incident.occurrence_count == 1 {
        "Occurred once".to_owned()
    } else {
        format!("Occurred {} times", incident.occurrence_count)
    };
    let metadata = format!(
        "{} · {} · {}: {} · {}: {}",
        crate::localization::translate_current(state_label),
        crate::localization::translate_current(&occurrence_label),
        crate::localization::translate_current("First"),
        format_system_time(incident.first_seen),
        crate::localization::translate_current("Latest"),
        format_system_time(incident.last_seen),
    );
    let detail_button_label = if incident.technical_detail_expanded {
        "Hide technical details"
    } else {
        "Show technical details"
    };
    let detail_button =
        button(container(localized_text(detail_button_label).size(11)).padding([4, 8]))
            .on_press(Message::SearchServiceIncidentDetailsToggled(incident.kind))
            .style(context_menu_button_style());

    let mut content = Column::new()
        .spacing(4)
        .width(Length::Fill)
        .push(localized_text(diagnostic_title(incident.kind)).size(13))
        .push(localized_text(diagnostic_recommendation(incident.kind)).size(11))
        .push(readable_text(metadata).size(10))
        .push(detail_button);

    if incident.technical_detail_expanded {
        let copy_button =
            button(container(localized_text("Copy technical details").size(11)).padding([4, 8]))
                .on_press(Message::SearchServiceIncidentDetailsCopyRequested(
                    incident.kind,
                ))
                .style(context_menu_button_style());
        content = content
            .push(
                readable_text(&incident.technical_detail)
                    .size(10)
                    .width(Length::Fill),
            )
            .push(copy_button);
    }

    info_setting_row(content.into())
}

fn service_recovery_rows(browser: &FileBrowser) -> Vec<Element<'static, Message>> {
    let recovery = &browser.search_service.recovery;
    let recovery_is_running = recovery.is_running();
    let restart_row = action_setting_row(
        "Restart Index Service",
        "Gracefully stop and restart the managed service.",
    );
    let restart_row = if recovery_is_running {
        restart_row
    } else {
        restart_row.on_press(Message::SearchServiceRestartRequested)
    };

    let force_restart_row = if *recovery == SearchServiceRecoveryState::ConfirmingForceRestart {
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
    if let Some(status_text) = recovery_status_text(recovery) {
        rows.push(status_notice(status_text));
    }
    rows
}

fn recovery_status_text(state: &SearchServiceRecoveryState) -> Option<&'static str> {
    match state {
        SearchServiceRecoveryState::Idle | SearchServiceRecoveryState::ConfirmingForceRestart => {
            None
        }
        SearchServiceRecoveryState::Running(SearchServiceRecoveryAction::Restart) => {
            Some("Restarting index service...")
        }
        SearchServiceRecoveryState::Running(SearchServiceRecoveryAction::ForceRestart) => {
            Some("Force restarting index service...")
        }
        SearchServiceRecoveryState::Succeeded(SearchServiceRecoveryAction::Restart) => {
            Some("Index service restarted successfully.")
        }
        SearchServiceRecoveryState::Succeeded(SearchServiceRecoveryAction::ForceRestart) => {
            Some("Index service force restarted successfully.")
        }
        SearchServiceRecoveryState::Failed { .. } => {
            Some("Index service restart failed. See Recent Issues for technical details.")
        }
    }
}

fn diagnostic_title(kind: SearchServiceDiagnosticKind) -> &'static str {
    match kind {
        SearchServiceDiagnosticKind::EndpointTimedOut => "Search service response is delayed",
        SearchServiceDiagnosticKind::EndpointUnavailable => "Cannot reach the search service",
        SearchServiceDiagnosticKind::ServiceChanging => "Search service is changing",
        SearchServiceDiagnosticKind::ServiceUnverified => "Cannot verify the search service",
        SearchServiceDiagnosticKind::EndpointInvalid => "Search service connection is inconsistent",
        SearchServiceDiagnosticKind::ComponentIncompatible => {
            "Search service components do not match"
        }
        SearchServiceDiagnosticKind::RecoveryFailed => "Could not restart the search service",
        SearchServiceDiagnosticKind::ServiceDegraded => "Search service needs attention",
        SearchServiceDiagnosticKind::ServiceFailed => "Search service stopped working",
        SearchServiceDiagnosticKind::IndexedQueriesUnavailable => {
            "Indexed search is temporarily unavailable"
        }
        SearchServiceDiagnosticKind::IndexFailed => "Index update failed",
        SearchServiceDiagnosticKind::IndexMaintenanceDegraded => {
            "Index maintenance needs attention"
        }
        SearchServiceDiagnosticKind::IndexMaintenanceFailed => "Index maintenance stopped",
    }
}

fn diagnostic_recommendation(kind: SearchServiceDiagnosticKind) -> &'static str {
    match kind {
        SearchServiceDiagnosticKind::EndpointTimedOut
        | SearchServiceDiagnosticKind::EndpointUnavailable
        | SearchServiceDiagnosticKind::ServiceChanging => {
            "Wait for automatic rechecking. If the problem continues, restart the index service."
        }
        SearchServiceDiagnosticKind::ServiceUnverified => {
            "Review the technical details. Restart the index service if the problem continues."
        }
        SearchServiceDiagnosticKind::EndpointInvalid => {
            "Restart the index service to establish a verified connection."
        }
        SearchServiceDiagnosticKind::ComponentIncompatible => {
            "Reinstall the search components from the current File Manager package."
        }
        SearchServiceDiagnosticKind::RecoveryFailed => {
            "Review the technical details. Use force restart only if the service remains unresponsive."
        }
        SearchServiceDiagnosticKind::ServiceDegraded
        | SearchServiceDiagnosticKind::IndexedQueriesUnavailable
        | SearchServiceDiagnosticKind::IndexMaintenanceDegraded => {
            "Search may use a slower fallback while the service recovers."
        }
        SearchServiceDiagnosticKind::ServiceFailed
        | SearchServiceDiagnosticKind::IndexFailed
        | SearchServiceDiagnosticKind::IndexMaintenanceFailed => {
            "Restart the index service. Existing index data and settings will be kept."
        }
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

#[cfg(test)]
mod tests {
    use file_search::{IndexedQueryAvailability, SearchServicePhase, SearchServiceStatus};

    use super::*;
    use crate::config;
    use crate::model::SearchServiceState;

    #[test]
    fn diagnostic_copy_uses_stable_user_facing_text() {
        assert_eq!(
            diagnostic_title(SearchServiceDiagnosticKind::EndpointTimedOut),
            "Search service response is delayed"
        );
        assert!(
            diagnostic_recommendation(SearchServiceDiagnosticKind::ComponentIncompatible)
                .contains("Reinstall")
        );
    }

    #[test]
    fn count_formatting_groups_large_index_totals() {
        assert_eq!(format_count(98_765), "98,765");
    }

    #[test]
    fn routine_background_refresh_does_not_change_overview_row_count() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        browser.search_service = SearchServiceState::new();
        let initial_request = browser.search_service.begin_initial_status_request();
        browser.search_service.accept_status_request(
            initial_request,
            Ok(SearchServiceStatus {
                phase: SearchServicePhase::Ready,
                query_availability: IndexedQueryAvailability::Available,
                index_status: None,
            }),
        );
        let settled_row_count = status_overview_rows(&browser).len();

        let refresh_request = browser.search_service.request_status_refresh();

        assert!(refresh_request.is_some());
        assert_eq!(status_overview_rows(&browser).len(), settled_row_count);
    }
}
