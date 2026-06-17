use std::path::PathBuf;
use std::time::{Duration, Instant};

use file_core::{DirectoryEntry, FileKind};
use iced::widget::image;

use crate::animated_image_preview::AnimatedImagePreview;
use crate::audio_preview::AudioPreviewRuntime;
use crate::text_preview::{TextPreviewFormat, TextPreviewLineLimitNotice};

#[derive(Debug, Clone)]
pub(crate) enum PreviewState {
    Loading(PathBuf),
    Ready(PreviewContent),
    Error(String),
}

#[derive(Debug, Clone)]
pub(crate) enum PreviewContent {
    Directory {
        entries: Vec<PreviewTreeEntry>,
    },
    Text {
        path: PathBuf,
        rendered: String,
        format: TextPreviewFormat,
        next_offset: Option<u64>,
        loaded_line_count: usize,
        line_limit_notice: Option<TextPreviewLineLimitNotice>,
    },
    Archive {
        entries: Vec<PreviewTreeEntry>,
    },
    Image {
        path: PathBuf,
        handle: image::Handle,
        width: u32,
        height: u32,
        max_edge: u32,
    },
    AnimatedImage(AnimatedImagePreview),
    Audio {
        path: PathBuf,
        duration: Option<Duration>,
        len: u64,
    },
    Video {
        path: PathBuf,
        frame: Option<image::Handle>,
        width: u32,
        height: u32,
        duration: Option<Duration>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct AudioPreviewPlayback {
    pub(crate) path: PathBuf,
    pub(crate) runtime: Option<AudioPreviewRuntime>,
    pub(crate) status: AudioPreviewPlaybackStatus,
    pub(crate) position: Duration,
    pub(crate) volume: f32,
    pub(crate) error: Option<String>,
}

impl AudioPreviewPlayback {
    pub(crate) fn loading(path: PathBuf) -> Self {
        Self {
            path,
            runtime: None,
            status: AudioPreviewPlaybackStatus::Loading,
            position: Duration::ZERO,
            volume: 1.0,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioPreviewPlaybackStatus {
    Loading,
    Playing,
    Paused,
    Stopped,
    Finished,
    Error,
}

#[derive(Debug, Clone)]
pub(crate) struct VideoPreviewPlayback {
    pub(crate) path: PathBuf,
    pub(crate) audio_runtime: Option<AudioPreviewRuntime>,
    pub(crate) status: VideoPreviewPlaybackStatus,
    pub(crate) position: Duration,
    // Subscription 的身份必须固定；播放进度 tick 只更新 position，不能重建 ffmpeg 流。
    pub(crate) stream_start_position: Duration,
    pub(crate) duration: Option<Duration>,
    pub(crate) volume: f32,
    pub(crate) generation: u64,
    pub(crate) seek_completion: Option<VideoPreviewSeekCompletion>,
    pub(crate) seek_frame_in_flight: Option<Duration>,
    pub(crate) pending_seek_frame: Option<Duration>,
    pub(crate) started_at: Option<Instant>,
    pub(crate) error: Option<String>,
}

impl VideoPreviewPlayback {
    pub(crate) fn playing(path: PathBuf, duration: Option<Duration>) -> Self {
        Self {
            path,
            audio_runtime: None,
            status: VideoPreviewPlaybackStatus::Playing,
            position: Duration::ZERO,
            stream_start_position: Duration::ZERO,
            duration,
            volume: 1.0,
            generation: 1,
            seek_completion: None,
            seek_frame_in_flight: None,
            pending_seek_frame: None,
            started_at: Some(Instant::now()),
            error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoPreviewSeekCompletion {
    ResumePlayback,
    StayPaused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoPreviewPlaybackStatus {
    Playing,
    Paused,
    Finished,
    Error,
}

#[derive(Debug, Clone)]
pub(crate) struct VideoPreviewFrame {
    pub(crate) path: PathBuf,
    pub(crate) generation: u64,
    pub(crate) position: Duration,
    pub(crate) handle: image::Handle,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PreviewSize {
    pub(crate) width: f32,
    pub(crate) height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewWindowProfile {
    Regular,
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone)]
pub(crate) struct PreviewTreeEntry {
    pub(crate) id: usize,
    pub(crate) name: String,
    pub(crate) kind: FileKind,
    pub(crate) depth: usize,
    pub(crate) parent: Option<usize>,
    pub(crate) filesystem_path: Option<PathBuf>,
    pub(crate) directory_children: Option<PreviewTreeDirectoryChildren>,
    pub(crate) is_expanded: bool,
    pub(crate) toggle_rotation_progress: f32,
}

impl PreviewTreeEntry {
    pub(crate) fn from_directory_entry(
        id: usize,
        entry: DirectoryEntry,
        depth: usize,
        parent: Option<usize>,
    ) -> Self {
        let kind = entry.kind;
        Self {
            id,
            name: entry.name().to_string_lossy().into_owned(),
            kind,
            depth,
            parent,
            filesystem_path: Some(entry.path),
            directory_children: preview_tree_directory_children(kind),
            is_expanded: false,
            toggle_rotation_progress: 0.0,
        }
    }

    pub(crate) fn is_directory(&self) -> bool {
        self.kind == FileKind::Directory
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreviewTreeDirectoryChildren {
    Pending,
    Loading,
    Loaded,
    Error(String),
}

fn preview_tree_directory_children(kind: FileKind) -> Option<PreviewTreeDirectoryChildren> {
    (kind == FileKind::Directory).then_some(PreviewTreeDirectoryChildren::Pending)
}
