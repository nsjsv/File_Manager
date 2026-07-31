use iced::widget::{button, column, container, progress_bar, row, scrollable, svg, Svg};
use iced::{Alignment, Element, Length};

use crate::app::smooth_scroll::{smooth_scroll_content, smooth_scroll_id};
use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction, context_menu_button_style,
    context_menu_style, error_notification_style, operation_queue_indicator_button_style,
    path_suggestion_item_style,
};
use crate::formatting::{format_file_size, format_middle_ellipsized_text};
use crate::model::{Message, ScrollbarRegion, ScrollbarVisibility};
use crate::operation_progress::{
    active_indeterminate_track_handle, static_indeterminate_track_handle,
};
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
    animation_frame: u8,
) -> Option<Element<'_, Message>> {
    let svg = Svg::new(svg::Handle::from_memory(
        indicator_svg(
            queue_indicator_ring(queue, animation_frame),
            queue_indicator_badge(queue),
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
    animation_frame: u8,
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
            tasks = tasks.push(operation_task_row(task, animation_frame));
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

fn operation_task_row(task: &FileOperationTask, animation_frame: u8) -> Element<'_, Message> {
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

    let progress: Element<'static, Message> = match task.progress.fraction() {
        Some(fraction) => progress_bar(0.0..=1.0, fraction)
            .girth(Length::Fixed(3.0))
            .into(),
        None if matches!(
            task.status,
            FileOperationStatus::Running | FileOperationStatus::Canceling
        ) =>
        {
            Svg::new(active_indeterminate_track_handle(animation_frame))
                .width(Length::Fill)
                .height(Length::Fixed(4.0))
                .into()
        }
        None => Svg::new(static_indeterminate_track_handle())
            .width(Length::Fill)
            .height(Length::Fixed(4.0))
            .into(),
    };

    let mut body = column![title, paths, progress]
        .spacing(4)
        .width(Length::Fill);

    if let Some(detail) = operation_progress_detail(task) {
        body = body.push(readable_text(detail).size(11).width(Length::Fill));
    }

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

fn operation_progress_detail(task: &FileOperationTask) -> Option<String> {
    let byte_detail = task.progress.bytes().map(|(completed_bytes, total_bytes)| {
        format!(
            "{} / {}",
            format_file_size(completed_bytes),
            format_file_size(total_bytes)
        )
    });
    let item_detail = task.progress.items().map(|(completed_items, total_items)| {
        if crate::localization::current_language_is_chinese() {
            format!("{completed_items} / {total_items} 项")
        } else {
            format!("{completed_items} / {total_items} items")
        }
    });

    match (byte_detail, item_detail) {
        (Some(bytes), Some(items)) => Some(format!("{bytes} | {items}")),
        (Some(bytes), None) => Some(bytes),
        (None, Some(items)) => Some(items),
        (None, None)
            if matches!(
                task.status,
                FileOperationStatus::Running | FileOperationStatus::Canceling
            ) =>
        {
            Some(if crate::localization::current_language_is_chinese() {
                "处理中...".to_owned()
            } else {
                "Processing...".to_owned()
            })
        }
        (None, None) => None,
    }
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

#[derive(Debug, Clone, Copy, PartialEq)]
enum QueueIndicatorRing {
    Hidden,
    Determinate {
        fraction: f32,
        tone: QueueIndicatorTone,
    },
    ActiveIndeterminate {
        animation_frame: u8,
    },
    PausedIndeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueIndicatorTone {
    Active,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueIndicatorBadge {
    Hidden,
    Count(usize),
    Error,
    Paused,
}

fn queue_indicator_ring(queue: &FileOperationQueue, animation_frame: u8) -> QueueIndicatorRing {
    if let Some(fraction) = queue.indicator_progress() {
        return QueueIndicatorRing::Determinate {
            fraction,
            tone: if queue.is_active_paused() {
                QueueIndicatorTone::Paused
            } else {
                QueueIndicatorTone::Active
            },
        };
    }
    if queue.is_active_paused() {
        QueueIndicatorRing::PausedIndeterminate
    } else if queue.has_active_task() {
        QueueIndicatorRing::ActiveIndeterminate { animation_frame }
    } else {
        QueueIndicatorRing::Hidden
    }
}

fn queue_indicator_badge(queue: &FileOperationQueue) -> QueueIndicatorBadge {
    if queue.unread_count() > 1 {
        QueueIndicatorBadge::Count(queue.unread_count().min(99))
    } else if queue.has_unread_failed_task() {
        QueueIndicatorBadge::Error
    } else if queue.is_active_paused() {
        QueueIndicatorBadge::Paused
    } else {
        QueueIndicatorBadge::Hidden
    }
}

fn indicator_svg(ring: QueueIndicatorRing, badge: QueueIndicatorBadge) -> String {
    let circumference = 2.0 * std::f32::consts::PI * 12.0;
    let progress_svg = match ring {
        QueueIndicatorRing::Hidden => String::new(),
        QueueIndicatorRing::Determinate { fraction, tone } => {
            let offset = circumference * (1.0 - fraction.clamp(0.0, 1.0));
            let stroke = match tone {
                QueueIndicatorTone::Active => "#3b82f6",
                QueueIndicatorTone::Paused => "#f59e0b",
            };
            format!(
                r#"<circle data-progress-kind="determinate" cx="15" cy="15" r="12" fill="none" stroke="{stroke}" stroke-width="2.6" stroke-linecap="round" stroke-dasharray="{circumference:.2}" stroke-dashoffset="{offset:.2}" transform="rotate(-90 15 15)"/>"#
            )
        }
        QueueIndicatorRing::ActiveIndeterminate { animation_frame } => {
            let dash_offset = animation_frame % 12;
            format!(
                r##"<circle data-progress-kind="indeterminate" cx="15" cy="15" r="12" fill="none" stroke="#3b82f6" stroke-width="2.6" stroke-linecap="round" stroke-dasharray="7 5" stroke-dashoffset="-{dash_offset}" transform="rotate(-90 15 15)"/>"##
            )
        }
        QueueIndicatorRing::PausedIndeterminate => r##"<circle data-progress-kind="indeterminate" cx="15" cy="15" r="12" fill="none" stroke="#f59e0b" stroke-width="2.6" stroke-linecap="round" stroke-dasharray="7 5" transform="rotate(-90 15 15)"/>"##.to_owned(),
    };

    let (badge_label, badge_fill) = match badge {
        QueueIndicatorBadge::Hidden => (None, "#2563eb"),
        QueueIndicatorBadge::Count(count) => (Some(count.to_string()), "#2563eb"),
        QueueIndicatorBadge::Error => (Some("!".to_owned()), "#ef4444"),
        QueueIndicatorBadge::Paused => (Some("||".to_owned()), "#f59e0b"),
    };
    let badge_svg = badge_label
        .map(|label| {
            format!(
                r##"<circle cx="23" cy="7" r="5.2" fill="{badge_fill}"/>
<text x="23" y="9.8" text-anchor="middle" font-family="sans-serif" font-size="7.4" font-weight="700" fill="#ffffff">{label}</text>"##
            )
        })
        .unwrap_or_default();

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
        let svg = indicator_svg(QueueIndicatorRing::Hidden, QueueIndicatorBadge::Count(1));

        assert!(!svg.contains("data-progress-kind"));
    }

    #[test]
    fn indicator_hides_badge_after_tasks_are_read() {
        let svg = indicator_svg(QueueIndicatorRing::Hidden, QueueIndicatorBadge::Hidden);

        assert!(!svg.contains("<text"));
    }

    #[test]
    fn active_indeterminate_indicator_has_no_completion_fraction() {
        let svg = indicator_svg(
            QueueIndicatorRing::ActiveIndeterminate { animation_frame: 0 },
            QueueIndicatorBadge::Hidden,
        );

        assert!(svg.contains(r#"data-progress-kind="indeterminate""#));
        assert!(svg.contains(r#"stroke-dasharray="7 5""#));
        assert!(!svg.contains("0.35"));
    }

    #[test]
    fn indeterminate_indicator_animation_changes_only_dash_phase() {
        let first = indicator_svg(
            QueueIndicatorRing::ActiveIndeterminate { animation_frame: 0 },
            QueueIndicatorBadge::Hidden,
        );
        let second = indicator_svg(
            QueueIndicatorRing::ActiveIndeterminate { animation_frame: 1 },
            QueueIndicatorBadge::Hidden,
        );

        assert!(first.contains(r#"stroke-dasharray="7 5""#));
        assert!(second.contains(r#"stroke-dasharray="7 5""#));
        assert_ne!(first, second);
    }
}
