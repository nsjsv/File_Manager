use std::path::PathBuf;
use std::time::{Duration, Instant};

use file_core::{CopyProgress, FileKind, ScanOptions};
use iced::futures::channel::mpsc::Sender as IcedSender;
use iced::futures::SinkExt;
use iced::Task;
use tokio_util::sync::CancellationToken;

use crate::animated_image_preview::load_animated_image_preview;
use crate::audio_preview::{start_audio_preview, start_audio_preview_at};
use crate::model::{
    Message, RemotePreviewCacheFinished, RemotePreviewCacheMessage, RemotePreviewCacheProgress,
};
use crate::original_image_preview::load_original_image_preview;
use crate::preview::{load_directory_preview_children, load_preview};
use crate::remote_preview_cache::{cache_remote_preview_file, RemotePreviewCacheRequest};
use crate::text_preview::TextPreviewChunkRequest;
use crate::text_preview_loading::load_text_preview_chunk;
use crate::video_preview::{inspect_video_preview_metadata, load_video_preview_frame};

const NETWORK_PREVIEW_CACHE_CHANNEL_SIZE: usize = 16;
const NETWORK_PREVIEW_PROGRESS_UI_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn preview_command(
    path: PathBuf,
    kind: FileKind,
    options: ScanOptions,
    max_file_bytes: u64,
) -> Task<Message> {
    let preview_path = path.clone();
    Task::perform(
        load_preview(path, kind, options, max_file_bytes),
        move |preview_outcome| Message::PreviewLoaded(preview_path.clone(), preview_outcome),
    )
}

pub(crate) fn remote_preview_cache_command(
    source_path: PathBuf,
    generation: u64,
    cache_dir: PathBuf,
    max_file_bytes: u64,
    cancel: CancellationToken,
) -> Task<Message> {
    let request =
        RemotePreviewCacheRequest::new(source_path.clone(), cache_dir, max_file_bytes, cancel);
    Task::stream(iced::stream::channel(
        NETWORK_PREVIEW_CACHE_CHANNEL_SIZE,
        async move |mut output| {
            let outcome =
                download_remote_preview_with_progress(request, generation, &mut output).await;
            let _ = output
                .send(Message::RemotePreviewCache(
                    RemotePreviewCacheMessage::Finished(RemotePreviewCacheFinished {
                        source_path,
                        generation,
                        outcome,
                    }),
                ))
                .await;
        },
    ))
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

pub(crate) fn image_preview_dimensions_command(path: PathBuf, generation: u64) -> Task<Message> {
    let image_path = path.clone();
    Task::perform(load_image_dimensions(path), move |dimensions| {
        Message::ImagePreviewDimensionsLoaded(image_path.clone(), generation, dimensions)
    })
}

pub(crate) fn original_image_preview_command(
    path: PathBuf,
    generation: u64,
    max_file_bytes: u64,
    placeholder_handle: Option<iced::widget::image::Handle>,
    cancellation: CancellationToken,
) -> Task<Message> {
    let image_path = path.clone();
    Task::perform(
        load_original_image_preview(path, max_file_bytes, cancellation, placeholder_handle),
        move |outcome| Message::OriginalImagePreviewLoaded(image_path.clone(), generation, outcome),
    )
}

async fn download_remote_preview_with_progress(
    request: RemotePreviewCacheRequest,
    generation: u64,
    output: &mut IcedSender<Message>,
) -> Result<PathBuf, String> {
    let source_path = request.source_path.clone();
    let (progress_sender, mut progress_receiver) = tokio::sync::mpsc::unbounded_channel();
    let cache = cache_remote_preview_file(request, progress_sender);
    tokio::pin!(cache);
    let mut latest_progress = None;
    let mut last_progress_sent_at = None;

    loop {
        tokio::select! {
            progress = progress_receiver.recv() => {
                if let Some(progress) = progress {
                    latest_progress = Some(progress);
                    let now = Instant::now();
                    if should_send_remote_preview_progress(last_progress_sent_at, now) {
                        if let Some(progress) = latest_progress.take() {
                            send_remote_preview_progress(output, &source_path, generation, progress).await;
                            last_progress_sent_at = Some(now);
                        }
                    }
                }
            }
            outcome = &mut cache => {
                if let Some(progress) = latest_progress.take() {
                    send_remote_preview_progress(output, &source_path, generation, progress).await;
                }
                return outcome;
            }
        }
    }
}

fn should_send_remote_preview_progress(last_sent_at: Option<Instant>, now: Instant) -> bool {
    match last_sent_at {
        Some(last_sent_at) => {
            now.duration_since(last_sent_at) >= NETWORK_PREVIEW_PROGRESS_UI_INTERVAL
        }
        None => true,
    }
}

async fn send_remote_preview_progress(
    output: &mut IcedSender<Message>,
    source_path: &PathBuf,
    generation: u64,
    progress: CopyProgress,
) {
    let _ = output
        .send(Message::RemotePreviewCache(
            RemotePreviewCacheMessage::Progress(RemotePreviewCacheProgress {
                source_path: source_path.clone(),
                generation,
                bytes_done: progress.bytes_done,
                bytes_total: progress.bytes_total,
            }),
        ))
        .await;
}

pub(crate) fn animated_image_preview_command(path: PathBuf, generation: u64) -> Task<Message> {
    let image_path = path.clone();
    Task::perform(
        load_animated_image_preview(path, generation),
        move |preview_outcome| {
            Message::AnimatedImagePreviewLoaded(image_path.clone(), generation, preview_outcome)
        },
    )
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
