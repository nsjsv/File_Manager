use std::path::PathBuf;
use std::time::Duration;

use file_core::{FileKind, ScanOptions};
use iced::Task;

use crate::animated_image_preview::load_animated_image_preview;
use crate::audio_preview::{start_audio_preview, start_audio_preview_at};
use crate::model::Message;
use crate::preview::{load_directory_preview_children, load_preview};
use crate::text_preview::TextPreviewChunkRequest;
use crate::text_preview_loading::load_text_preview_chunk;
use crate::video_preview::{inspect_video_preview_metadata, load_video_preview_frame};

pub(crate) fn preview_command(
    path: PathBuf,
    kind: FileKind,
    options: ScanOptions,
) -> Task<Message> {
    let preview_path = path.clone();
    Task::perform(load_preview(path, kind, options), move |preview_outcome| {
        Message::PreviewLoaded(preview_path.clone(), preview_outcome)
    })
}

pub(crate) fn preview_directory_children_command(
    path: PathBuf,
    options: ScanOptions,
) -> Task<Message> {
    let parent_path = path.clone();
    Task::perform(
        load_directory_preview_children(path, options),
        move |children_outcome| {
            Message::PreviewDirectoryChildrenLoaded(parent_path.clone(), children_outcome)
        },
    )
}

pub(crate) fn startup_index_directory_children_command(
    path: PathBuf,
    request_generation: u64,
    options: ScanOptions,
) -> Task<Message> {
    let parent_path = path.clone();
    Task::perform(
        load_directory_preview_children(path, options),
        move |children_outcome| {
            Message::StartupIndexDirectoryChildrenLoaded(
                request_generation,
                parent_path.clone(),
                children_outcome,
            )
        },
    )
}

pub(crate) fn image_preview_dimensions_command(path: PathBuf) -> Task<Message> {
    let image_path = path.clone();
    Task::perform(load_image_dimensions(path), move |dimensions| {
        Message::ImagePreviewDimensionsLoaded(image_path.clone(), dimensions)
    })
}

pub(crate) fn animated_image_preview_command(path: PathBuf) -> Task<Message> {
    let image_path = path.clone();
    Task::perform(load_animated_image_preview(path), move |preview_outcome| {
        Message::AnimatedImagePreviewLoaded(image_path.clone(), preview_outcome)
    })
}

pub(crate) fn text_preview_chunk_command(request: TextPreviewChunkRequest) -> Task<Message> {
    let path = request.path.clone();
    let generation = request.generation;
    let start_offset = request.start_offset;
    Task::perform(load_text_preview_chunk(request), move |chunk_outcome| {
        Message::TextPreviewChunkLoaded {
            path: path.clone(),
            generation,
            start_offset,
            outcome: chunk_outcome,
        }
    })
}

pub(crate) fn start_audio_preview_command(path: PathBuf) -> Task<Message> {
    let audio_path = path.clone();
    Task::perform(start_audio_preview(path), move |playback_outcome| {
        Message::AudioPreviewStarted(audio_path.clone(), playback_outcome)
    })
}

pub(crate) fn start_video_preview_audio_command(
    path: PathBuf,
    generation: u64,
    position: Duration,
) -> Task<Message> {
    let video_path = path.clone();
    Task::perform(
        start_audio_preview_at(path, position),
        move |audio_outcome| {
            Message::VideoPreviewAudioStarted(video_path.clone(), generation, audio_outcome)
        },
    )
}

pub(crate) fn video_preview_metadata_command(path: PathBuf) -> Task<Message> {
    let video_path = path.clone();
    Task::perform(
        async move {
            inspect_video_preview_metadata(path)
                .await
                .map(|metadata| metadata.duration)
        },
        move |metadata_outcome| {
            Message::VideoPreviewMetadataLoaded(video_path.clone(), metadata_outcome)
        },
    )
}

pub(crate) fn video_preview_frame_command(
    path: PathBuf,
    generation: u64,
    position: Duration,
) -> Task<Message> {
    let video_path = path.clone();
    Task::perform(
        load_video_preview_frame(path, generation, position),
        move |frame_outcome| match frame_outcome {
            Ok(frame) => Message::VideoPreviewFrameLoaded(frame),
            Err(error) => Message::VideoPreviewSeekFrameFailed(
                video_path.clone(),
                generation,
                position,
                error,
            ),
        },
    )
}

async fn load_image_dimensions(path: PathBuf) -> Result<(u32, u32), String> {
    thumbnails::load_image_dimensions(path)
        .await
        .map_err(|error| error.to_string())
}
