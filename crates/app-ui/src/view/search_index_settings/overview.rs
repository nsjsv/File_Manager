use std::path::PathBuf;

use file_index::FileSearchIndexStatus;
use iced::widget::{column, container, row, Column};
use iced::{Alignment, Element, Length};

use crate::app::FileBrowser;
use crate::appearance::path_suggestion_item_style;
use crate::formatting::{format_file_size, format_middle_ellipsized_text};
use crate::model::{Message, SearchIndexSettingsSection};
use crate::typography::readable_text;

use super::{
    action_button, format_unix_ms, metadata_row, root_action_row, root_action_row_without_status,
    section_panel, ROOT_PATH_MAX_CHARS,
};

pub(super) fn search_index_overview_content(browser: &FileBrowser) -> Column<'_, Message> {
    column![
        search_index_navigation_panel(browser),
        compact_root_statuses_panel(browser),
    ]
    .spacing(12)
    .width(Length::Fill)
}

fn search_index_navigation_panel(browser: &FileBrowser) -> Element<'_, Message> {
    let issue_report = browser.search_index_issue_report();
    let path_rule_count = browser.search_index.path_rule_entries().len();

    section_panel(
        column![
            readable_text("Details").size(13),
            metadata_row("Issues", issue_summary(&issue_report)),
            metadata_row("Path rules", path_rule_count.to_string()),
            row![
                action_button(
                    "View errors",
                    Some(Message::SearchIndexSettingsSectionSelected(
                        SearchIndexSettingsSection::Errors,
                    )),
                ),
                action_button(
                    "Path rules",
                    Some(Message::SearchIndexSettingsSectionSelected(
                        SearchIndexSettingsSection::PathRules,
                    )),
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        ]
        .spacing(8),
    )
}

fn compact_root_statuses_panel<'a>(browser: &'a FileBrowser) -> Element<'a, Message> {
    let roots = browser.search_index_setting_roots();
    let mut statuses = Column::new()
        .spacing(8)
        .push(readable_text("Indexed roots").size(13));

    if roots.is_empty() {
        statuses = statuses.push(readable_text("No searchable roots are available yet.").size(12));
    } else {
        for root in roots {
            statuses = statuses.push(compact_root_row(browser, root));
        }
    }

    section_panel(statuses)
}

fn compact_root_row<'a>(browser: &'a FileBrowser, root: PathBuf) -> Element<'a, Message> {
    let is_loading = browser
        .search_index
        .status_loading_roots
        .contains_key(&root);
    let is_indexing = browser.search_index.indexing_roots.contains(&root);
    let status = browser.search_index.statuses.get(&root);
    let error = browser.search_index.root_errors.get(&root);

    let root_label = root.to_string_lossy();
    let root_label = format_middle_ellipsized_text(root_label.as_ref(), ROOT_PATH_MAX_CHARS);
    let summary = compact_root_summary(status, is_loading, is_indexing, error);
    let actions = match status {
        Some(status) => root_action_row(
            root.clone(),
            is_loading || is_indexing,
            status.exists,
            !status.failures.is_empty(),
        ),
        None => root_action_row_without_status(root.clone(), is_loading || is_indexing),
    };

    container(
        column![
            readable_text(root_label).size(13),
            readable_text(summary).size(11),
            actions,
        ]
        .spacing(6)
        .width(Length::Fill),
    )
    .padding([6, 8])
    .width(Length::Fill)
    .style(path_suggestion_item_style)
    .into()
}

fn compact_root_summary(
    status: Option<&FileSearchIndexStatus>,
    is_loading: bool,
    is_indexing: bool,
    error: Option<&String>,
) -> String {
    let mut parts = Vec::new();

    if error.is_some() {
        parts.push("Error".to_owned());
    }
    if is_indexing {
        parts.push("Indexing".to_owned());
    } else if is_loading {
        parts.push("Loading".to_owned());
    }

    if let Some(status) = status {
        parts.push(compact_status_label(status).to_owned());
        parts.push(format!("{} records", status.record_count));
        parts.push(format_file_size(status.index_size_bytes));
        if status.failed_count > 0 {
            parts.push(format!("{} failures", status.failed_count));
        }
        if let Some(updated_at_ms) = status.updated_at_ms.or(status.built_at_ms) {
            parts.push(format!("Updated {}", format_unix_ms(updated_at_ms)));
        }
    }

    if parts.is_empty() {
        "No index yet".to_owned()
    } else {
        parts.join(" · ")
    }
}

fn compact_status_label(status: &FileSearchIndexStatus) -> &'static str {
    if status.stale {
        "Needs rebuild"
    } else if status.exists {
        "Present"
    } else {
        "Missing"
    }
}

fn issue_summary(report: &crate::app::search_index_settings::SearchIndexIssueReport) -> String {
    match report.issue_count() {
        0 => "No issues".to_owned(),
        1 => "1 issue".to_owned(),
        count => format!("{count} issues"),
    }
}
