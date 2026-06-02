use std::path::Path;
use std::time::Duration;

use iced::widget::{
    button, column, container, image, mouse_area, row, scrollable, slider, text, text_editor,
    Button, Column, Space,
};
use iced::{Alignment, Element, Font, Length};

use crate::appearance::{
    auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction,
    navigation_icon_button_style, preview_panel_style, text_preview_editor_style,
};
use crate::formatting::{format_duration, format_file_size, format_middle_ellipsized_text};
use crate::icons::{preview_entry_icon_symbol, rotated_chevron_right_view, IconSymbol};
use crate::model::{
    AudioPreviewPlayback, AudioPreviewPlaybackStatus, Message, PreviewContent, PreviewSize,
    PreviewState, PreviewTreeEntry, ScrollbarVisibility, TextPreviewDocument, TextPreviewFormat,
    VideoPreviewPlayback, VideoPreviewPlaybackStatus,
};
use crate::preview::PREVIEW_TEXT_LIMIT;
use crate::typography::readable_text;

use super::{auxiliary_window_message, icon_tone_style, themed_icon, IconTone};

const PREVIEW_HEADER_RESERVED_HEIGHT: f32 = 96.0;
const PREVIEW_MIN_SCROLL_HEIGHT: f32 = 160.0;
const PREVIEW_ICON_SIZE: f32 = 16.0;
const PREVIEW_PATH_MAX_CHARS: usize = 64;
const PREVIEW_ENTRY_NAME_MAX_CHARS: usize = 48;
const PREVIEW_TREE_INDENT_WIDTH: f32 = 18.0;
const PREVIEW_TREE_TOGGLE_WIDTH: f32 = 16.0;
const PREVIEW_TREE_TOGGLE_ROTATION_DEGREES: f32 = 90.0;
const AUDIO_PREVIEW_CONTROL_HEIGHT: f32 = 92.0;
const AUDIO_CONTROL_BUTTON_SIZE: f32 = 30.0;
const AUDIO_CONTROL_ICON_SIZE: f32 = 14.0;
const AUDIO_TIMELINE_CONTROL_GAP: f32 = 10.0;
const AUDIO_PROGRESS_SLIDER_WIDTH: f32 = 280.0;
const AUDIO_PROGRESS_SLIDER_STEP_SECONDS: f32 = 0.05;
const AUDIO_VOLUME_SLIDER_WIDTH: f32 = 100.0;
const AUDIO_VOLUME_SLIDER_STEP: f32 = 0.01;
const VIDEO_PREVIEW_CONTROL_HEIGHT: f32 = 88.0;
const VIDEO_PROGRESS_SLIDER_PORTION: u16 = 3;
const VIDEO_VOLUME_SLIDER_PORTION: u16 = 1;
const VIDEO_CONTROL_SLIDER_GAP: f32 = 14.0;
const VIDEO_VOLUME_ICON_GAP: f32 = 6.0;

pub(crate) fn view_preview_window<'a>(
    preview: Option<&'a PreviewState>,
    text_preview_document: Option<&'a TextPreviewDocument>,
    size: PreviewSize,
    audio_preview: Option<&'a AudioPreviewPlayback>,
    video_preview: Option<&'a VideoPreviewPlayback>,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'a, Message> {
    preview
        .map(|preview| {
            preview_panel(
                preview,
                text_preview_document,
                size,
                audio_preview,
                video_preview,
                scrollbar_visibility,
            )
        })
        .unwrap_or_else(|| {
            auxiliary_window_message("Select a file and press Space to load preview")
        })
}

fn preview_panel<'a>(
    preview: &'a PreviewState,
    text_preview_document: Option<&'a TextPreviewDocument>,
    size: PreviewSize,
    audio_preview: Option<&'a AudioPreviewPlayback>,
    video_preview: Option<&'a VideoPreviewPlayback>,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'a, Message> {
    let scroll_height = preview_scroll_height(size);
    let panel = match preview {
        PreviewState::Loading(path) => column![
            readable_text(preview_title(path)).size(14),
            readable_text("Loading preview...").size(14),
        ],
        PreviewState::Ready(PreviewContent::Directory {
            path,
            entries,
            total,
            skipped,
            truncated,
        }) => directory_preview_panel(
            path,
            entries,
            *total,
            *skipped,
            *truncated,
            scroll_height,
            scrollbar_visibility,
        ),
        PreviewState::Ready(PreviewContent::Text {
            path,
            format,
            truncated,
            ..
        }) => text_preview_panel(
            path,
            *format,
            *truncated,
            text_preview_document.filter(|document| document.path() == path.as_path()),
            scroll_height,
        ),
        PreviewState::Ready(PreviewContent::Archive {
            path,
            entries,
            total,
            truncated,
        }) => archive_preview_panel(
            path,
            entries,
            *total,
            *truncated,
            scroll_height,
            scrollbar_visibility,
        ),
        PreviewState::Ready(PreviewContent::Image {
            handle,
            width,
            height,
            ..
        }) => return image_preview_panel(handle, *width, *height, size),
        PreviewState::Ready(PreviewContent::Audio {
            path,
            duration,
            len,
        }) => audio_preview_panel(path, *duration, *len, audio_preview),
        PreviewState::Ready(PreviewContent::Video {
            path,
            frame,
            width,
            height,
            duration,
            ..
        }) => {
            return video_preview_panel(
                path,
                frame.as_ref(),
                *width,
                *height,
                *duration,
                video_preview,
                size,
            )
        }
        PreviewState::Error(error) => column![
            readable_text("Preview").size(14),
            readable_text(error).size(14),
        ],
    }
    .spacing(6);

    let content = panel.height(Length::Fill);

    container(content)
        .padding(14)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(preview_panel_style)
        .into()
}

fn preview_scroll_height(size: PreviewSize) -> f32 {
    (size.height - PREVIEW_HEADER_RESERVED_HEIGHT).max(PREVIEW_MIN_SCROLL_HEIGHT)
}

fn directory_preview_panel(
    path: &std::path::PathBuf,
    entries: &[PreviewTreeEntry],
    total: usize,
    skipped: usize,
    truncated: bool,
    scroll_height: f32,
    scrollbar_visibility: ScrollbarVisibility,
) -> Column<'static, Message> {
    let listing = preview_tree_listing(entries, "Empty directory");

    column![
        readable_text(preview_title(path)).size(14),
        text(directory_preview_summary(
            entries.len(),
            total,
            skipped,
            truncated
        ))
        .size(14),
        scrollable(listing)
            .direction(preview_scroll_direction(scrollbar_visibility))
            .style(auto_hide_scrollbar_style(scrollbar_visibility))
            .height(Length::Fixed(scroll_height)),
    ]
}

fn directory_preview_summary(
    loaded_count: usize,
    total: usize,
    skipped: usize,
    truncated: bool,
) -> String {
    let summary = if truncated {
        format!("Showing first {loaded_count} entries")
    } else {
        format!("{total} entries")
    };

    if skipped > 0 {
        format!("{summary}, {skipped} skipped")
    } else {
        summary
    }
}

fn archive_preview_panel(
    path: &std::path::PathBuf,
    entries: &[PreviewTreeEntry],
    total: usize,
    truncated: bool,
    scroll_height: f32,
    scrollbar_visibility: ScrollbarVisibility,
) -> Column<'static, Message> {
    let listing = preview_tree_listing(entries, "Empty archive");

    column![
        readable_text(preview_title(path)).size(14),
        readable_text(archive_preview_summary(entries.len(), total, truncated)).size(14),
        scrollable(listing)
            .direction(preview_scroll_direction(scrollbar_visibility))
            .style(auto_hide_scrollbar_style(scrollbar_visibility))
            .height(Length::Fixed(scroll_height)),
    ]
}

fn archive_preview_summary(loaded_count: usize, total: usize, truncated: bool) -> String {
    if truncated {
        format!("Showing first {loaded_count} of {total} entries")
    } else {
        format!("{total} entries")
    }
}

fn preview_tree_listing(
    entries: &[PreviewTreeEntry],
    empty_message: &'static str,
) -> Column<'static, Message> {
    let mut listing = Column::new().spacing(3);
    if entries.is_empty() {
        return listing.push(readable_text(empty_message).size(14));
    }

    for entry in visible_preview_tree_entries(entries) {
        listing = listing.push(preview_tree_entry_row(entry));
    }

    listing
}

fn visible_preview_tree_entries(entries: &[PreviewTreeEntry]) -> Vec<&PreviewTreeEntry> {
    entries
        .iter()
        .filter(|entry| preview_tree_entry_visible(entry, entries))
        .collect()
}

fn preview_tree_entry_visible(entry: &PreviewTreeEntry, entries: &[PreviewTreeEntry]) -> bool {
    let mut parent = entry.parent;
    while let Some(parent_id) = parent {
        let Some(parent_entry) = entries.get(parent_id) else {
            return false;
        };
        if !(parent_entry.is_expanded || parent_entry.toggle_rotation_progress > 0.0) {
            return false;
        }
        parent = parent_entry.parent;
    }

    true
}

fn preview_tree_entry_row(entry: &PreviewTreeEntry) -> Element<'static, Message> {
    let name = format_middle_ellipsized_text(&entry.name, PREVIEW_ENTRY_NAME_MAX_CHARS);
    let indent = Space::with_width(Length::Fixed(
        entry.depth as f32 * PREVIEW_TREE_INDENT_WIDTH,
    ));
    let toggle: Element<'static, Message> = if entry.is_directory() {
        container(
            rotated_chevron_right_view(
                entry.toggle_rotation_progress * PREVIEW_TREE_TOGGLE_ROTATION_DEGREES,
                PREVIEW_TREE_TOGGLE_WIDTH,
            )
            .style(icon_tone_style(IconTone::Normal)),
        )
        .width(Length::Fixed(PREVIEW_TREE_TOGGLE_WIDTH))
        .height(Length::Fixed(PREVIEW_TREE_TOGGLE_WIDTH))
        .center_x()
        .center_y()
        .into()
    } else {
        Space::with_width(Length::Fixed(PREVIEW_TREE_TOGGLE_WIDTH)).into()
    };
    let row_content = row![
        indent,
        toggle,
        themed_icon(
            preview_entry_icon_symbol(entry.kind, &entry.name),
            IconTone::Normal,
            PREVIEW_ICON_SIZE,
        ),
        readable_text(name).size(14).width(Length::Fill),
    ]
    .spacing(6)
    .align_items(Alignment::Center);
    let row_container = container(row_content).padding([3, 6]).width(Length::Fill);

    if entry.is_directory() {
        mouse_area(row_container)
            .on_press(Message::PreviewTreeDirectoryToggled(entry.id))
            .interaction(iced::mouse::Interaction::Pointer)
            .into()
    } else {
        row_container.into()
    }
}

fn text_preview_panel<'a>(
    path: &'a std::path::PathBuf,
    format: TextPreviewFormat,
    truncated: bool,
    document: Option<&'a TextPreviewDocument>,
    scroll_height: f32,
) -> Column<'a, Message> {
    let status = text_preview_status(format, truncated);
    let body: Element<'_, Message> = if let Some(document) = document {
        text_editor(document.content())
            .height(Length::Fixed(scroll_height))
            .font(Font::MONOSPACE)
            .padding(8)
            .style(text_preview_editor_style())
            .on_action(Message::TextPreviewAction)
            .into()
    } else {
        readable_text("Text preview is not ready").size(14).into()
    };

    column![
        readable_text(preview_title(path)).size(14),
        readable_text(status).size(14),
        body,
    ]
}

fn text_preview_status(format: TextPreviewFormat, truncated: bool) -> String {
    let label = match format {
        TextPreviewFormat::Plain => "Selectable text preview",
        TextPreviewFormat::Markdown => "Rendered Markdown preview",
    };
    let copy_hint = "drag to select, Ctrl+C to copy";

    if truncated {
        format!(
            "{label} · showing first {} · {copy_hint}",
            format_file_size(PREVIEW_TEXT_LIMIT as u64)
        )
    } else {
        format!("{label} · {copy_hint}")
    }
}

fn image_preview_panel(
    handle: &image::Handle,
    width: u32,
    height: u32,
    size: PreviewSize,
) -> Element<'static, Message> {
    let (image_width, image_height) = image_preview_size(size, width, height);
    container(
        image::Image::new(handle.clone())
            .width(Length::Fixed(image_width))
            .height(Length::Fixed(image_height)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x()
    .center_y()
    .into()
}

fn image_preview_size(size: PreviewSize, width: u32, height: u32) -> (f32, f32) {
    let max_width = size.width.max(1.0);
    let max_height = size.height.max(1.0);
    scaled_media_size(max_width, max_height, width, height)
}

fn audio_preview_panel(
    path: &std::path::PathBuf,
    duration: Option<Duration>,
    len: u64,
    playback: Option<&AudioPreviewPlayback>,
) -> Column<'static, Message> {
    let playback = playback.filter(|playback| playback.path.as_path() == path.as_path());
    let title = column![
        readable_text("Audio preview").size(17),
        readable_text(audio_preview_summary(duration, len)).size(12),
        readable_text(audio_preview_status(playback, duration)).size(12),
    ]
    .spacing(4)
    .width(Length::Fixed(150.0));

    let controls = row![
        title,
        audio_timeline_control(playback, duration),
        audio_volume_control(playback),
    ]
    .spacing(12)
    .align_items(Alignment::Center);

    column![
        readable_text(preview_title(path)).size(14),
        container(controls)
            .width(Length::Fill)
            .height(Length::Fixed(AUDIO_PREVIEW_CONTROL_HEIGHT))
            .center_y(),
    ]
}

fn audio_primary_button(playback: Option<&AudioPreviewPlayback>) -> Button<'static, Message> {
    let icon = match playback.map(|playback| playback.status) {
        Some(AudioPreviewPlaybackStatus::Playing) => IconSymbol::Pause,
        _ => IconSymbol::Play,
    };
    let button = button(themed_icon(icon, IconTone::Normal, AUDIO_CONTROL_ICON_SIZE))
        .padding(8)
        .width(Length::Fixed(AUDIO_CONTROL_BUTTON_SIZE))
        .height(Length::Fixed(AUDIO_CONTROL_BUTTON_SIZE))
        .style(navigation_icon_button_style());
    if matches!(
        playback.map(|playback| playback.status),
        Some(AudioPreviewPlaybackStatus::Loading)
    ) {
        button
    } else {
        button.on_press(Message::AudioPreviewPlaybackToggled)
    }
}

fn audio_timeline_control(
    playback: Option<&AudioPreviewPlayback>,
    duration: Option<Duration>,
) -> Element<'static, Message> {
    let position = playback
        .map(|playback| playback.position)
        .unwrap_or(Duration::ZERO);
    let duration_seconds = duration
        .map(|duration| duration.as_secs_f32())
        .unwrap_or_else(|| (position.as_secs_f32() + 1.0).max(1.0))
        .max(1.0);
    let position_seconds = position.as_secs_f32().min(duration_seconds);

    let slider_row = row![
        audio_primary_button(playback),
        slider(
            0.0..=duration_seconds,
            position_seconds,
            Message::AudioPreviewSeekRequested,
        )
        .step(AUDIO_PROGRESS_SLIDER_STEP_SECONDS)
        .width(Length::Fixed(AUDIO_PROGRESS_SLIDER_WIDTH)),
    ]
    .spacing(AUDIO_TIMELINE_CONTROL_GAP)
    .align_items(Alignment::Center);
    let label_offset = AUDIO_CONTROL_BUTTON_SIZE + AUDIO_TIMELINE_CONTROL_GAP;

    column![
        slider_row,
        row![
            Space::with_width(Length::Fixed(label_offset)),
            readable_text(audio_position_text(position, duration)).size(12),
        ],
    ]
    .spacing(4)
    .width(Length::Fixed(label_offset + AUDIO_PROGRESS_SLIDER_WIDTH))
    .into()
}

fn audio_volume_control(playback: Option<&AudioPreviewPlayback>) -> Element<'static, Message> {
    let volume = playback.map(|playback| playback.volume).unwrap_or(1.0);
    column![
        readable_text(format!("Volume {:.0}%", volume * 100.0)).size(12),
        slider(0.0..=1.0, volume, Message::AudioPreviewVolumeChanged)
            .step(AUDIO_VOLUME_SLIDER_STEP)
            .width(Length::Fixed(AUDIO_VOLUME_SLIDER_WIDTH)),
    ]
    .spacing(4)
    .width(Length::Fixed(AUDIO_VOLUME_SLIDER_WIDTH))
    .into()
}

fn audio_preview_summary(duration: Option<Duration>, len: u64) -> String {
    match duration {
        Some(duration) => format!("{} · {}", format_duration(duration), format_file_size(len)),
        None => format!("Duration unknown · {}", format_file_size(len)),
    }
}

fn audio_preview_status(
    playback: Option<&AudioPreviewPlayback>,
    duration: Option<Duration>,
) -> String {
    let Some(playback) = playback else {
        return "Ready to play".to_owned();
    };

    match playback.status {
        AudioPreviewPlaybackStatus::Loading => "Opening audio output...".to_owned(),
        AudioPreviewPlaybackStatus::Playing => {
            format!(
                "Playing · {}",
                audio_position_text(playback.position, duration)
            )
        }
        AudioPreviewPlaybackStatus::Paused => {
            format!(
                "Paused · {}",
                audio_position_text(playback.position, duration)
            )
        }
        AudioPreviewPlaybackStatus::Stopped => "Stopped".to_owned(),
        AudioPreviewPlaybackStatus::Finished => "Finished".to_owned(),
        AudioPreviewPlaybackStatus::Error => playback
            .error
            .clone()
            .unwrap_or_else(|| "Could not start audio preview".to_owned()),
    }
}

fn audio_position_text(position: Duration, duration: Option<Duration>) -> String {
    match duration {
        Some(duration) => format!(
            "{} / {}",
            format_duration(position),
            format_duration(duration)
        ),
        None => format_duration(position),
    }
}

fn video_preview_panel(
    path: &std::path::PathBuf,
    frame: Option<&image::Handle>,
    width: u32,
    height: u32,
    duration: Option<Duration>,
    playback: Option<&VideoPreviewPlayback>,
    size: PreviewSize,
) -> Element<'static, Message> {
    let playback = playback.filter(|playback| playback.path.as_path() == path.as_path());
    let (frame_width, frame_height) = video_frame_size(size, width, height);
    let frame_content: Element<'static, Message> = if let Some(frame) = frame {
        image::Image::new(frame.clone())
            .width(Length::Fixed(frame_width))
            .height(Length::Fixed(frame_height))
            .into()
    } else {
        container(Space::with_width(Length::Fixed(frame_width)))
            .width(Length::Fixed(frame_width))
            .height(Length::Fixed(frame_height))
            .center_x()
            .center_y()
            .into()
    };
    let frame_view: Element<'static, Message> = container(frame_content)
        .width(Length::Fill)
        .height(Length::Fixed(frame_height))
        .center_x()
        .center_y()
        .into();
    let controls = video_controls(playback, duration, frame_width);

    container(
        column![
            frame_view,
            container(controls)
                .width(Length::Fill)
                .height(Length::Fixed(VIDEO_PREVIEW_CONTROL_HEIGHT))
                .center_x()
                .center_y(),
        ]
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x()
    .center_y()
    .into()
}

fn video_frame_size(size: PreviewSize, width: u32, height: u32) -> (f32, f32) {
    let max_width = size.width.max(1.0);
    let max_height = (size.height - VIDEO_PREVIEW_CONTROL_HEIGHT).max(1.0);
    scaled_media_size(max_width, max_height, width, height)
}

fn scaled_media_size(max_width: f32, max_height: f32, width: u32, height: u32) -> (f32, f32) {
    if width == 0 || height == 0 {
        return (max_width, max_height);
    }

    let aspect_ratio = width as f32 / height as f32;
    let mut frame_width = max_width;
    let mut frame_height = frame_width / aspect_ratio;
    if frame_height > max_height {
        frame_height = max_height;
        frame_width = frame_height * aspect_ratio;
    }

    (frame_width.max(1.0), frame_height.max(1.0))
}

fn video_primary_button(playback: Option<&VideoPreviewPlayback>) -> Button<'static, Message> {
    let icon = match playback.map(|playback| playback.status) {
        Some(VideoPreviewPlaybackStatus::Playing) => IconSymbol::Pause,
        _ => IconSymbol::Play,
    };
    button(themed_icon(icon, IconTone::Normal, AUDIO_CONTROL_ICON_SIZE))
        .on_press(Message::VideoPreviewPlaybackToggled)
        .padding(8)
        .width(Length::Fixed(AUDIO_CONTROL_BUTTON_SIZE))
        .height(Length::Fixed(AUDIO_CONTROL_BUTTON_SIZE))
        .style(navigation_icon_button_style())
}

fn video_controls(
    playback: Option<&VideoPreviewPlayback>,
    duration: Option<Duration>,
    width: f32,
) -> Element<'static, Message> {
    let position = playback
        .map(|playback| playback.position)
        .unwrap_or(Duration::ZERO);
    let duration = playback.and_then(|playback| playback.duration).or(duration);
    let duration_seconds = duration
        .map(|duration| duration.as_secs_f32())
        .unwrap_or_else(|| (position.as_secs_f32() + 1.0).max(1.0))
        .max(1.0);
    let position_seconds = position.as_secs_f32().min(duration_seconds);

    let slider_row = row![
        slider(
            0.0..=duration_seconds,
            position_seconds,
            Message::VideoPreviewSeekRequested,
        )
        .step(AUDIO_PROGRESS_SLIDER_STEP_SECONDS)
        .on_release(Message::VideoPreviewSeekCommitted)
        .width(Length::FillPortion(VIDEO_PROGRESS_SLIDER_PORTION)),
        container(video_volume_control(playback))
            .width(Length::FillPortion(VIDEO_VOLUME_SLIDER_PORTION)),
    ]
    .spacing(VIDEO_CONTROL_SLIDER_GAP)
    .width(Length::Fixed(width))
    .align_items(Alignment::Center);

    column![
        row![
            video_primary_button(playback),
            readable_text(audio_position_text(position, duration)).size(12),
        ]
        .spacing(AUDIO_TIMELINE_CONTROL_GAP)
        .align_items(Alignment::Center),
        slider_row,
    ]
    .spacing(8)
    .width(Length::Fixed(width))
    .into()
}

fn video_volume_control(playback: Option<&VideoPreviewPlayback>) -> Element<'static, Message> {
    row![
        themed_icon(
            IconSymbol::Volume2,
            IconTone::Normal,
            AUDIO_CONTROL_ICON_SIZE
        ),
        video_volume_slider(playback).width(Length::Fill),
    ]
    .spacing(VIDEO_VOLUME_ICON_GAP)
    .align_items(Alignment::Center)
    .into()
}

fn video_volume_slider(
    playback: Option<&VideoPreviewPlayback>,
) -> iced::widget::Slider<'static, f32, Message> {
    let volume = playback.map(|playback| playback.volume).unwrap_or(1.0);
    slider(0.0..=1.0, volume, Message::VideoPreviewVolumeChanged).step(AUDIO_VOLUME_SLIDER_STEP)
}

fn preview_title(path: &Path) -> String {
    let path = path.to_string_lossy();
    let path = format_middle_ellipsized_text(path.as_ref(), PREVIEW_PATH_MAX_CHARS);
    format!("Preview: {path}")
}

fn preview_scroll_direction(
    scrollbar_visibility: ScrollbarVisibility,
) -> iced::widget::scrollable::Direction {
    auto_hide_vertical_scrollbar_direction(scrollbar_visibility, 6.0)
}
