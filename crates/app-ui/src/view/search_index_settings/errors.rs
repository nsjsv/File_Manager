use crate::app::search_index_settings::{search_index_display_path, SearchIndexIssueRoot};
use crate::app::FileBrowser;
use crate::formatting::format_middle_ellipsized_text;
use crate::model::{Message, SearchIndexErrorCopyTarget, SearchIndexSettingsSection};
use crate::typography::readable_text;
use iced::widget::{column, row, Column};
use iced::{Alignment, Element, Length};

use super::{
    action_button, format_unix_ms, section_panel, FAILURE_PATH_MAX_CHARS, ROOT_PATH_MAX_CHARS,
};

pub(super) fn search_index_errors_content(browser: &FileBrowser) -> Column<'_, Message> {
    let report = browser.search_index_issue_report();
    let mut content = column![errors_header(!report.is_empty())]
        .spacing(12)
        .width(Length::Fill);

    if report.is_empty() {
        return content.push(section_panel(column![readable_text(
            "No errors or failures are recorded for the current profile."
        )
        .size(12),]));
    }

    if report.daemon_error.is_some() || report.profile_error.is_some() {
        content = content.push(global_errors_panel(&report));
    }

    for root in &report.roots {
        content = content.push(root_errors_panel(browser, root));
    }

    content
}

fn errors_header(can_copy_all: bool) -> Element<'static, Message> {
    row![
        readable_text("Errors and failures")
            .size(14)
            .width(Length::Fill),
        action_button(
            "Back",
            Some(Message::SearchIndexSettingsSectionSelected(
                SearchIndexSettingsSection::Overview,
            )),
        ),
        action_button(
            "Copy all",
            can_copy_all.then_some(Message::SearchIndexErrorCopyRequested(
                SearchIndexErrorCopyTarget::All,
            )),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn global_errors_panel(
    report: &crate::app::search_index_settings::SearchIndexIssueReport,
) -> Element<'static, Message> {
    let mut content = column![readable_text("Service and profile").size(13)];

    if let Some(error) = &report.daemon_error {
        content = content.push(error_item(
            "Service error",
            error,
            SearchIndexErrorCopyTarget::DaemonStatus,
        ));
    }
    if let Some(error) = &report.profile_error {
        content = content.push(error_item(
            "Profile error",
            error,
            SearchIndexErrorCopyTarget::ProfileError,
        ));
    }

    section_panel(content)
}

fn root_errors_panel<'a>(
    browser: &'a FileBrowser,
    root: &SearchIndexIssueRoot,
) -> Element<'a, Message> {
    let root_label = search_index_display_path(&root.root, &browser.search_index_home_directory());
    let root_label = format_middle_ellipsized_text(&root_label, ROOT_PATH_MAX_CHARS);
    let mut content = column![row![
        readable_text(root_label).size(13).width(Length::Fill),
        readable_text(root_issue_summary(root)).size(11),
    ]
    .spacing(8)
    .align_y(Alignment::Center),];

    if let Some(error) = &root.root_error {
        content = content.push(error_item(
            "Root error",
            error,
            SearchIndexErrorCopyTarget::RootError(root.root.clone()),
        ));
    }

    for (index, failure) in root.failures.iter().enumerate() {
        let failure_path =
            search_index_display_path(&failure.path, &browser.search_index_home_directory());
        let failure_path = format_middle_ellipsized_text(&failure_path, FAILURE_PATH_MAX_CHARS);
        let detail = format!(
            "{}\nLast failed {}",
            failure.message,
            format_unix_ms(failure.last_failed_at_ms)
        );
        content = content.push(error_item(
            &failure_path,
            &detail,
            SearchIndexErrorCopyTarget::Failure {
                root: root.root.clone(),
                index,
            },
        ));
    }

    section_panel(content)
}

fn root_issue_summary(root: &SearchIndexIssueRoot) -> String {
    match (root.root_error.is_some(), root.failures.len()) {
        (true, 0) => "1 root error".to_owned(),
        (false, 1) => "1 failure".to_owned(),
        (false, count) => format!("{count} failures"),
        (true, 1) => "1 root error · 1 failure".to_owned(),
        (true, count) => format!("1 root error · {count} failures"),
    }
}

fn error_item(
    title: &str,
    detail: &str,
    target: SearchIndexErrorCopyTarget,
) -> Element<'static, Message> {
    row![
        column![
            readable_text(title.to_owned()).size(12),
            readable_text(detail.to_owned()).size(12),
        ]
        .spacing(3)
        .width(Length::Fill),
        action_button("Copy", Some(Message::SearchIndexErrorCopyRequested(target)),),
    ]
    .spacing(8)
    .align_y(Alignment::Start)
    .into()
}
