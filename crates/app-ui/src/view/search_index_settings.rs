use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use file_core::{FileSearchIndexFailure, FileSearchIndexMode, FileSearchIndexStatus};
use iced::widget::{button, column, container, row, text, text_input, Button, Column, Space, Text};
use iced::{Alignment, Element, Length, Theme};

use crate::app::FileBrowser;
use crate::appearance::{context_menu_button_style, path_suggestion_item_style};
use crate::formatting::{format_file_size, format_middle_ellipsized_text, format_system_time};
use crate::model::Message;
use crate::typography::readable_text;

const ROOT_PATH_MAX_CHARS: usize = 72;
const INDEX_DIR_MAX_CHARS: usize = 76;
const FAILURE_PATH_MAX_CHARS: usize = 78;

pub(super) fn search_index_settings_content(browser: &FileBrowser) -> Column<'_, Message> {
    column![
        settings_header(),
        index_directory_panel(browser),
        exclude_patterns_panel(browser),
        root_statuses_panel(browser),
    ]
    .spacing(12)
    .width(Length::Fill)
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

fn exclude_patterns_panel<'a>(browser: &'a FileBrowser) -> Element<'a, Message> {
    let mut patterns: Column<'a, Message> = Column::new()
        .spacing(8)
        .push(scoped_text("Exclude rules").size(13))
        .push(scoped_text("These patterns apply to future searches and index builds.").size(12));

    if browser.search_index.exclude_pattern_inputs.is_empty() {
        patterns = patterns.push(scoped_text("No exclude rules configured.").size(12));
    } else {
        for (index, pattern) in browser
            .search_index
            .exclude_pattern_inputs
            .iter()
            .enumerate()
        {
            patterns = patterns.push(exclude_pattern_row(index, pattern));
        }
    }

    let save_message = browser
        .search_index_exclude_patterns_have_changes()
        .then_some(Message::SearchIndexExcludePatternsSaved);
    patterns = patterns.push(
        row![
            action_button("Add rule", Some(Message::SearchIndexExcludePatternAdded)),
            action_button("Save rules", save_message),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );

    section_panel(patterns)
}

fn exclude_pattern_row<'a>(index: usize, pattern: &'a str) -> Element<'a, Message> {
    row![
        text_input("Pattern", pattern)
            .on_input(move |value| Message::SearchIndexExcludePatternChanged(index, value))
            .padding([6, 8])
            .size(13)
            .width(Length::Fill),
        action_button(
            "Delete",
            Some(Message::SearchIndexExcludePatternRemoved(index)),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
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
    let is_loading = browser.search_index.status_loading_roots.contains(&root);
    let is_indexing = browser.search_index.indexing_roots.contains(&root);
    let status = browser.search_index.statuses.get(&root);
    let error = browser.search_index.errors.get(&root);
    let index_dir = browser.search_index_dir_for_settings_root(&root);

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
        details = details.push(readable_text(format!("Status error: {error}")).size(12));
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
    let state_label = if status.exists { "Present" } else { "Missing" };
    let last_update = status
        .updated_at_ms
        .or(status.built_at_ms)
        .map(format_unix_ms)
        .unwrap_or_else(|| "Never".to_owned());

    column![
        metadata_row("State", state_label.to_owned()),
        metadata_row("Records", status.record_count.to_string()),
        metadata_row("Size", format_file_size(status.index_size_bytes)),
        metadata_row("Last update", last_update),
        metadata_row("Failures", status.failed_count.to_string()),
    ]
    .spacing(4)
    .into()
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
