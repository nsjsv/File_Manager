use iced::widget::{button, column, container, progress_bar, row, scrollable, svg, Svg};
use iced::{Alignment, Element, Length};

use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, context_menu_button_style,
    context_menu_style, error_notification_style, operation_queue_indicator_button_style,
    path_suggestion_item_style,
};
use crate::formatting::format_middle_ellipsized_text;
use crate::model::{Message, ScrollbarRegion, ScrollbarVisibility};
use crate::operation_queue::{FileOperationQueue, FileOperationStatus, FileOperationTask};
use crate::operation_queue_display::FileOperationPathLines;
use crate::typography::readable_text;

pub(crate) const OPERATION_QUEUE_PANEL_WIDTH: f32 = 360.0;
pub(crate) const OPERATION_QUEUE_PANEL_BOTTOM: f32 = 18.0;
pub(crate) const OPERATION_QUEUE_INDICATOR_SIZE: f32 = 30.0;
pub(crate) const OPERATION_QUEUE_INDICATOR_RIGHT: f32 = 14.0;
pub(crate) const OPERATION_QUEUE_INDICATOR_BOTTOM: f32 = 14.0;

const TASK_FILE_NAME_MAX_CHARS: usize = 24;
const TASK_PATH_MAX_CHARS: usize = 46;
const TASK_ERROR_MAX_CHARS: usize = 120;
const TASK_LIST_MAX_HEIGHT: f32 = 320.0;

pub(crate) fn operation_queue_indicator(
    queue: &FileOperationQueue,
) -> Option<Element<'_, Message>> {
    let svg = Svg::new(svg::Handle::from_memory(
        indicator_svg(
            queue.indicator_progress(),
            queue.unread_count(),
            queue.has_active_task(),
            queue.has_unread_failed_task(),
            queue.is_active_paused(),
        )
        .into_bytes(),
    ))
    .width(Length::Fixed(OPERATION_QUEUE_INDICATOR_SIZE))
    .height(Length::Fixed(OPERATION_QUEUE_INDICATOR_SIZE));

    Some(
        button(svg)
            .on_press(Message::FileOperationIndicatorPressed)
            .padding(0)
            .style(operation_queue_indicator_button_style())
            .into(),
    )
}

pub(crate) fn operation_queue_panel(
    queue: &FileOperationQueue,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    let header = row![
        readable_text("Tasks").size(16).width(Length::Fill),
        readable_text(if crate::localization::current_language_is_chinese() {
            format!("{} 个任务", queue.task_count())
        } else {
            format!("{} tasks", queue.task_count())
        })
        .size(12),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut tasks = column![].spacing(6);
    if queue.tasks().is_empty() {
        tasks = tasks.push(empty_queue_row());
    } else {
        for task in queue.tasks() {
            tasks = tasks.push(operation_task_row(task));
        }
    }

    let scroll_region = ScrollbarRegion::OperationQueue;
    let content = column![
        header,
        scrollable(smooth_scroll_content(tasks, scroll_region.clone()))
            .id(smooth_scroll_id(&scroll_region))
            .direction(auto_hide_vertical_scrollbar_direction(
                scrollbar_visibility,
                6.0,
            ))
            .style(auto_hide_scrollbar_style(scrollbar_visibility))
            .height(Length::Fixed(TASK_LIST_MAX_HEIGHT))
            .on_scroll(|_| Message::OperationQueueScrolled),
    ]
    .spacing(10);

    container(content)
        .padding(12)
        .width(Length::Fixed(OPERATION_QUEUE_PANEL_WIDTH))
        .style(context_menu_style)
        .into()
}

fn operation_task_row(task: &FileOperationTask) -> Element<'_, Message> {
    let path_lines = task.operation.path_lines();
    let title = row![
        readable_text(operation_title_text(task.operation.title(), &path_lines))
            .size(13)
            .width(Length::Fill),
        readable_text(task.status.label()).size(11),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let paths = column![
        readable_text(path_line_text("Original", &path_lines.original_path)).size(11),
        readable_text(path_line_text("Directory", &path_lines.directory_path)).size(11),
    ]
    .spacing(2)
    .width(Length::Fill);

    let progress =
        progress_bar(0.0..=1.0, task.progress.display_fraction()).girth(Length::Fixed(3.0));

    let mut body = column![title, paths, progress]
        .spacing(4)
        .width(Length::Fill);

    if let Some(error) = task.error.as_deref() {
        let error = crate::localization::translate_current(error);
        body = body.push(
            readable_text(format_middle_ellipsized_text(&error, TASK_ERROR_MAX_CHARS))
                .size(11)
                .width(Length::Fill),
        );
    }

    let mut content = column![body].spacing(6);
    if let Some(controls) = operation_task_controls(task) {
        content = content.push(controls);
    }
    let item = container(content).padding(8).width(Length::Fill);
    let item = if task.status == FileOperationStatus::Failed {
        item.style(error_notification_style)
    } else {
        item.style(path_suggestion_item_style)
    };

    item.into()
}

fn operation_title_text(title: &str, path_lines: &FileOperationPathLines) -> String {
    let file_name = format_middle_ellipsized_text(&path_lines.file_name, TASK_FILE_NAME_MAX_CHARS);
    let more_count = path_lines.total_items.saturating_sub(1);
    let title = crate::localization::translate_current(title);
    if more_count == 0 {
        format!("{title} - {file_name}")
    } else {
        format!("{title} - {file_name} (+{more_count})")
    }
}

fn path_line_text(label: &'static str, path: &str) -> String {
    let path = format_middle_ellipsized_text(path, TASK_PATH_MAX_CHARS);
    format!("{}: {path}", crate::localization::translate_current(label))
}

fn empty_queue_row() -> Element<'static, Message> {
    container(readable_text("No tasks").size(13).width(Length::Fill))
        .padding(10)
        .width(Length::Fill)
        .style(path_suggestion_item_style)
        .into()
}

fn operation_task_controls(task: &FileOperationTask) -> Option<Element<'static, Message>> {
    let pause_label = match task.status {
        FileOperationStatus::Paused => "Resume",
        _ => "Pause",
    };
    let can_pause = matches!(
        task.status,
        FileOperationStatus::Running | FileOperationStatus::Paused
    ) && task.operation.supports_pause();
    let can_cancel = matches!(
        task.status,
        FileOperationStatus::Pending | FileOperationStatus::Running | FileOperationStatus::Paused
    );

    if !can_pause && !can_cancel {
        return None;
    }

    let mut controls = row![].spacing(6).align_y(Alignment::Center);
    if can_pause {
        controls = controls.push(task_control_button(
            pause_label,
            Message::FileOperationPauseToggled(task.id),
        ));
    }
    if can_cancel {
        controls = controls.push(task_control_button(
            "Cancel",
            Message::FileOperationCancelRequested(task.id),
        ));
    }

    Some(controls.into())
}

fn task_control_button(label: &'static str, message: Message) -> Element<'static, Message> {
    button(readable_text(label).size(12))
        .on_press(message)
        .padding([4, 8])
        .style(context_menu_button_style())
        .into()
}

fn indicator_svg(
    progress: Option<f32>,
    unread_count: usize,
    has_active_task: bool,
    has_unread_error: bool,
    is_paused: bool,
) -> String {
    let has_visible_progress =
        has_active_task || has_unread_error || is_paused || progress.is_some();
    let progress = progress
        .unwrap_or(if has_visible_progress { 0.35 } else { 0.0 })
        .clamp(0.0, 1.0);
    let circumference = 2.0 * std::f32::consts::PI * 12.0;
    let offset = circumference * (1.0 - progress);
    let stroke = if has_unread_error {
        "#ef4444"
    } else if is_paused {
        "#f59e0b"
    } else {
        "#3b82f6"
    };
    let badge = if unread_count > 1 {
        Some(unread_count.min(99).to_string())
    } else if has_unread_error {
        Some("!".to_owned())
    } else if is_paused {
        Some("||".to_owned())
    } else {
        None
    };
    let badge_fill = if has_unread_error {
        "#ef4444"
    } else if is_paused {
        "#f59e0b"
    } else {
        "#2563eb"
    };
    let badge_svg = if let Some(label) = badge {
        format!(
            r##"<circle cx="23" cy="7" r="5.2" fill="{badge_fill}"/>
<text x="23" y="9.8" text-anchor="middle" font-family="sans-serif" font-size="7.4" font-weight="700" fill="#ffffff">{label}</text>"##
        )
    } else {
        String::new()
    };
    let progress_svg = if has_visible_progress {
        format!(
            r#"<circle cx="15" cy="15" r="12" fill="none" stroke="{stroke}" stroke-width="2.6" stroke-linecap="round" stroke-dasharray="{circumference:.2}" stroke-dashoffset="{offset:.2}" transform="rotate(-90 15 15)"/>"#
        )
    } else {
        String::new()
    };

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="30" viewBox="0 0 30 30">
<circle cx="15" cy="15" r="14" fill="#111827" fill-opacity="0.78"/>
<circle cx="15" cy="15" r="12" fill="none" stroke="#64748b" stroke-opacity="0.34" stroke-width="2.6"/>
{progress_svg}
<g transform="translate(6.4 6.4) scale(0.72)" fill="none" stroke="#f8fafc" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
<rect x="3" y="5" width="6" height="6" rx="1"/>
<path d="m3 17 2 2 4-4"/>
<path d="M13 6h8"/>
<path d="M13 12h8"/>
<path d="M13 18h8"/>
</g>
{badge_svg}
</svg>"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicator_hides_progress_ring_for_completed_only_tasks() {
        let svg = indicator_svg(None, 1, false, false, false);

        assert!(!svg.contains("stroke-dasharray"));
    }

    #[test]
    fn indicator_hides_badge_after_tasks_are_read() {
        let svg = indicator_svg(None, 0, false, false, false);

        assert!(!svg.contains("<text"));
    }

    #[test]
    fn indicator_keeps_progress_ring_for_active_indeterminate_task() {
        let svg = indicator_svg(None, 1, true, false, false);

        assert!(svg.contains("stroke-dasharray"));
    }
}
