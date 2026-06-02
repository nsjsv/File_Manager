use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iced::Command;

use super::FileBrowser;
use crate::commands::{
    start_audio_preview_command, start_video_preview_audio_command, video_preview_frame_command,
    video_preview_metadata_command,
};
use crate::model::{
    AudioPreviewPlayback, AudioPreviewPlaybackStatus, Message, PreviewContent, PreviewState,
    PreviewTreeEntry, PreviewWindowProfile, VideoPreviewFrame, VideoPreviewPlayback,
    VideoPreviewPlaybackStatus, VideoPreviewSeekCompletion,
};

const PREVIEW_TREE_TOGGLE_ROTATION_STEP: f32 = 0.18;
const PREVIEW_TREE_TOGGLE_ROTATION_EPSILON: f32 = 0.001;

impl FileBrowser {
    pub(super) fn accept_preview(
        &mut self,
        path: PathBuf,
        preview_outcome: Result<PreviewContent, String>,
    ) -> Command<Message> {
        let is_active_preview_request = matches!(
            &self.preview,
            Some(PreviewState::Loading(loading_path)) if loading_path == &path
        );
        if !is_active_preview_request {
            return Command::none();
        }

        match preview_outcome {
            Ok(preview) => {
                let command = match &preview {
                    PreviewContent::Audio { .. } => {
                        self.clear_video_preview();
                        Command::none()
                    }
                    PreviewContent::Video { path, duration, .. } => {
                        self.clear_audio_preview();
                        self.start_video_preview_playback(path.clone(), *duration)
                    }
                    _ => {
                        self.clear_audio_preview();
                        self.clear_video_preview();
                        Command::none()
                    }
                };
                self.preview = Some(PreviewState::Ready(preview));
                self.error = None;
                command
            }
            Err(error) => {
                self.clear_audio_preview();
                self.clear_video_preview();
                self.preview = Some(PreviewState::Error(error));
                if self.preview_window.is_none() {
                    self.ensure_preview_window(PreviewWindowProfile::Video)
                } else {
                    Command::none()
                }
            }
        }
    }

    pub(super) fn accept_video_preview_frame(
        &mut self,
        video_frame: VideoPreviewFrame,
    ) -> Command<Message> {
        let frame_path = video_frame.path.clone();
        let frame_generation = video_frame.generation;
        let frame_position = video_frame.position;
        let Some(playback) = self.video_preview.as_ref() else {
            return Command::none();
        };
        if playback.path != video_frame.path || playback.generation != video_frame.generation {
            return Command::none();
        }

        let Some(PreviewState::Ready(PreviewContent::Video {
            path,
            frame: current_frame,
            width,
            height,
            ..
        })) = &mut self.preview
        else {
            return Command::none();
        };
        if path != &video_frame.path {
            return Command::none();
        }

        let should_resize_window = *width != video_frame.width || *height != video_frame.height;
        *current_frame = Some(video_frame.handle);
        *width = video_frame.width;
        *height = video_frame.height;
        let resize_command = if should_resize_window {
            self.fit_preview_window_to_video_frame(video_frame.width, video_frame.height)
        } else {
            Command::none()
        };
        Command::batch([
            resize_command,
            self.finish_video_seek_frame_decode(frame_path, frame_generation, frame_position),
        ])
    }

    pub(super) fn accept_video_preview_metadata(
        &mut self,
        path: PathBuf,
        metadata_outcome: Result<Option<Duration>, String>,
    ) -> Command<Message> {
        let Ok(metadata_duration) = metadata_outcome else {
            return Command::none();
        };
        let Some(PreviewState::Ready(PreviewContent::Video {
            path: preview_path,
            duration,
            ..
        })) = &mut self.preview
        else {
            return Command::none();
        };
        if preview_path != &path {
            return Command::none();
        }

        *duration = metadata_duration;
        if let Some(playback) = self
            .video_preview
            .as_mut()
            .filter(|playback| playback.path == path)
        {
            playback.duration = metadata_duration;
            playback.position = clamp_video_preview_position(playback.position, metadata_duration);
        }
        Command::none()
    }

    pub(super) fn accept_video_preview_error(
        &mut self,
        path: PathBuf,
        generation: u64,
        error: String,
    ) -> Command<Message> {
        let matches_playback = self
            .video_preview
            .as_ref()
            .is_some_and(|playback| playback.path == path && playback.generation == generation);
        if !matches_playback {
            return Command::none();
        }

        if let Some(playback) = self.video_preview.as_mut() {
            finish_video_preview_audio(playback);
            playback.status = VideoPreviewPlaybackStatus::Error;
            playback.seek_completion = None;
            playback.seek_frame_in_flight = None;
            playback.pending_seek_frame = None;
            playback.error = Some(error.clone());
        }
        if active_video_preview_path(self.preview.as_ref()) == Some(path.as_path()) {
            self.preview = Some(PreviewState::Error(error));
            if self.preview_window.is_none() {
                return self.ensure_preview_window(PreviewWindowProfile::Video);
            }
        }
        Command::none()
    }

    pub(super) fn accept_video_preview_seek_frame_error(
        &mut self,
        path: PathBuf,
        generation: u64,
        position: Duration,
        error: String,
    ) -> Command<Message> {
        let Some(playback) = self
            .video_preview
            .as_mut()
            .filter(|playback| playback.path == path && playback.generation == generation)
        else {
            return Command::none();
        };

        if playback.seek_frame_in_flight != Some(position) {
            return Command::none();
        }
        playback.seek_frame_in_flight = None;
        if let Some(next_position) = playback.pending_seek_frame.take() {
            return start_video_seek_frame_decode(playback, next_position);
        }
        playback.error = Some(error);
        Command::none()
    }

    pub(super) fn accept_video_preview_finished(
        &mut self,
        path: PathBuf,
        generation: u64,
    ) -> Command<Message> {
        let Some(playback) = self
            .video_preview
            .as_mut()
            .filter(|playback| playback.path == path && playback.generation == generation)
        else {
            return Command::none();
        };

        refresh_video_preview_position(playback);
        finish_video_preview_audio(playback);
        playback.status = VideoPreviewPlaybackStatus::Finished;
        playback.seek_completion = None;
        playback.seek_frame_in_flight = None;
        playback.pending_seek_frame = None;
        playback.started_at = None;
        if let Some(duration) = playback.duration {
            playback.position = duration;
        }
        Command::none()
    }

    pub(super) fn active_video_preview_stream(&self) -> Option<(PathBuf, u64, Duration)> {
        let playback = self.video_preview.as_ref()?;
        if playback.status != VideoPreviewPlaybackStatus::Playing {
            return None;
        }
        if active_video_preview_path(self.preview.as_ref())? != playback.path.as_path() {
            return None;
        }

        Some((
            playback.path.clone(),
            playback.generation,
            playback.position,
        ))
    }

    pub(super) fn toggle_preview_tree_directory(&mut self, entry_id: usize) -> Command<Message> {
        let Some(PreviewState::Ready(
            PreviewContent::Directory { entries, .. } | PreviewContent::Archive { entries, .. },
        )) = &mut self.preview
        else {
            return Command::none();
        };
        let Some(entry) = entries.get_mut(entry_id) else {
            return Command::none();
        };
        if entry.is_directory() {
            entry.is_expanded = !entry.is_expanded;
        }

        Command::none()
    }

    pub(super) fn preview_tree_animation_is_active(&self) -> bool {
        let Some(entries) = preview_tree_entries(self.preview.as_ref()) else {
            return false;
        };
        entries.iter().any(preview_tree_rotation_is_active)
    }

    pub(super) fn advance_preview_tree_animation(&mut self) -> Command<Message> {
        let Some(entries) = preview_tree_entries_mut(self.preview.as_mut()) else {
            return Command::none();
        };

        for entry in entries.iter_mut().filter(|entry| entry.is_directory()) {
            let target = preview_tree_rotation_target(entry);
            if entry.toggle_rotation_progress < target {
                entry.toggle_rotation_progress = (entry.toggle_rotation_progress
                    + PREVIEW_TREE_TOGGLE_ROTATION_STEP)
                    .min(target);
            } else if entry.toggle_rotation_progress > target {
                entry.toggle_rotation_progress = (entry.toggle_rotation_progress
                    - PREVIEW_TREE_TOGGLE_ROTATION_STEP)
                    .max(target);
            }
        }

        Command::none()
    }

    pub(super) fn toggle_audio_preview_playback(&mut self) -> Command<Message> {
        let Some(path) = active_audio_preview_path(self.preview.as_ref()).map(Path::to_path_buf)
        else {
            return Command::none();
        };

        if let Some(playback) = self
            .audio_preview
            .as_mut()
            .filter(|playback| playback.path == path)
        {
            match playback.status {
                AudioPreviewPlaybackStatus::Loading => return Command::none(),
                AudioPreviewPlaybackStatus::Playing => {
                    if let Some(runtime) = &playback.runtime {
                        playback.position = runtime.position();
                        runtime.pause();
                        playback.status = AudioPreviewPlaybackStatus::Paused;
                        return Command::none();
                    }
                }
                AudioPreviewPlaybackStatus::Paused => {
                    if let Some(runtime) = &playback.runtime {
                        runtime.play();
                        playback.status = AudioPreviewPlaybackStatus::Playing;
                        playback.error = None;
                        return Command::none();
                    }
                }
                AudioPreviewPlaybackStatus::Stopped
                | AudioPreviewPlaybackStatus::Finished
                | AudioPreviewPlaybackStatus::Error => {}
            }
        }

        self.start_audio_preview_playback(path)
    }

    pub(super) fn accept_audio_preview_started(
        &mut self,
        path: PathBuf,
        playback_outcome: Result<crate::audio_preview::AudioPreviewRuntime, String>,
    ) -> Command<Message> {
        let is_current_audio_preview = active_audio_preview_path(self.preview.as_ref())
            .is_some_and(|preview_path| preview_path == path.as_path());
        let Some(playback) = self
            .audio_preview
            .as_mut()
            .filter(|playback| playback.path == path)
        else {
            return Command::none();
        };
        if !is_current_audio_preview || playback.status != AudioPreviewPlaybackStatus::Loading {
            return Command::none();
        }

        match playback_outcome {
            Ok(runtime) => {
                runtime.set_volume(playback.volume);
                playback.position = runtime.position();
                playback.runtime = Some(runtime);
                playback.status = AudioPreviewPlaybackStatus::Playing;
                playback.error = None;
            }
            Err(error) => {
                playback.runtime = None;
                playback.status = AudioPreviewPlaybackStatus::Error;
                playback.error = Some(error);
            }
        }

        Command::none()
    }

    pub(super) fn seek_audio_preview_playback(
        &mut self,
        position_seconds: f32,
    ) -> Command<Message> {
        let Some(playback) = self.audio_preview.as_mut() else {
            return Command::none();
        };
        let Some(runtime) = playback.runtime.as_ref() else {
            return Command::none();
        };

        let position = Duration::from_secs_f32(position_seconds.max(0.0));
        match runtime.seek_to(position) {
            Ok(()) => {
                playback.position = position;
                playback.error = None;
            }
            Err(error) => {
                playback.error = Some(error.clone());
                self.error = Some(error);
            }
        }

        Command::none()
    }

    pub(super) fn change_audio_preview_volume(&mut self, volume: f32) -> Command<Message> {
        let Some(playback) = self.audio_preview.as_mut() else {
            return Command::none();
        };
        let volume = volume.clamp(0.0, 1.0);
        playback.volume = volume;
        if let Some(runtime) = playback.runtime.as_ref() {
            runtime.set_volume(volume);
        }
        Command::none()
    }

    pub(super) fn update_audio_preview_playback(&mut self) -> Command<Message> {
        let Some(playback) = self.audio_preview.as_mut() else {
            return Command::none();
        };
        if playback.status != AudioPreviewPlaybackStatus::Playing {
            return Command::none();
        }
        let Some(runtime) = playback.runtime.as_ref() else {
            playback.status = AudioPreviewPlaybackStatus::Stopped;
            return Command::none();
        };

        playback.position = runtime.position();
        if runtime.is_finished() {
            runtime.stop();
            playback.runtime = None;
            playback.status = AudioPreviewPlaybackStatus::Finished;
        }

        Command::none()
    }

    pub(super) fn audio_preview_is_active(&self) -> bool {
        self.audio_preview.as_ref().is_some_and(|playback| {
            playback.status == AudioPreviewPlaybackStatus::Playing && playback.runtime.is_some()
        })
    }

    pub(super) fn toggle_video_preview_playback(&mut self) -> Command<Message> {
        let Some(status) = self.video_preview.as_ref().map(|playback| playback.status) else {
            return Command::none();
        };

        match status {
            VideoPreviewPlaybackStatus::Playing => {
                let Some(playback) = self.video_preview.as_mut() else {
                    return Command::none();
                };
                refresh_video_preview_position(playback);
                if let Some(runtime) = playback.audio_runtime.as_ref() {
                    runtime.pause();
                }
                playback.status = VideoPreviewPlaybackStatus::Paused;
                playback.seek_completion = None;
                playback.seek_frame_in_flight = None;
                playback.pending_seek_frame = None;
                playback.started_at = None;
                Command::none()
            }
            VideoPreviewPlaybackStatus::Paused | VideoPreviewPlaybackStatus::Error => {
                self.resume_video_preview_playback()
            }
            VideoPreviewPlaybackStatus::Finished => {
                let Some(playback) = self.video_preview.as_mut() else {
                    return Command::none();
                };
                playback.position = Duration::ZERO;
                self.resume_video_preview_playback()
            }
        }
    }

    pub(super) fn accept_video_preview_audio_started(
        &mut self,
        path: PathBuf,
        generation: u64,
        audio_outcome: Result<crate::audio_preview::AudioPreviewRuntime, String>,
    ) -> Command<Message> {
        let Some(playback) = self
            .video_preview
            .as_mut()
            .filter(|playback| playback.path == path && playback.generation == generation)
        else {
            if let Ok(runtime) = audio_outcome {
                runtime.stop();
            }
            return Command::none();
        };

        if playback.status != VideoPreviewPlaybackStatus::Playing {
            if let Ok(runtime) = audio_outcome {
                runtime.stop();
            }
            return Command::none();
        }

        match audio_outcome {
            Ok(runtime) => {
                runtime.set_volume(playback.volume);
                playback.audio_runtime = Some(runtime);
                playback.error = None;
            }
            Err(error) => {
                playback.audio_runtime = None;
                playback.error = Some(format!("Audio unavailable: {error}"));
            }
        }
        Command::none()
    }

    pub(super) fn seek_video_preview_playback(
        &mut self,
        position_seconds: f32,
    ) -> Command<Message> {
        let Some(playback) = self.video_preview.as_mut() else {
            return Command::none();
        };
        if playback.seek_completion.is_none() {
            playback.seek_completion = Some(match playback.status {
                VideoPreviewPlaybackStatus::Playing => VideoPreviewSeekCompletion::ResumePlayback,
                VideoPreviewPlaybackStatus::Paused
                | VideoPreviewPlaybackStatus::Finished
                | VideoPreviewPlaybackStatus::Error => VideoPreviewSeekCompletion::StayPaused,
            });
            playback.seek_frame_in_flight = None;
            playback.pending_seek_frame = None;
            playback.generation = playback.generation.saturating_add(1);
        }
        if playback.status == VideoPreviewPlaybackStatus::Playing {
            if let Some(runtime) = playback.audio_runtime.as_ref() {
                runtime.pause();
            }
        }
        let position = Duration::from_secs_f32(position_seconds.max(0.0));
        playback.position = clamp_video_preview_position(position, playback.duration);
        playback.status = VideoPreviewPlaybackStatus::Paused;
        playback.started_at = None;
        playback.error = None;
        if playback.seek_frame_in_flight.is_some() {
            playback.pending_seek_frame = Some(playback.position);
            Command::none()
        } else {
            let seek_position = playback.position;
            start_video_seek_frame_decode(playback, seek_position)
        }
    }

    pub(super) fn commit_video_preview_seek(&mut self) -> Command<Message> {
        let Some(playback) = self.video_preview.as_mut() else {
            return Command::none();
        };
        let seek_completion = playback.seek_completion.take();
        match seek_completion {
            Some(VideoPreviewSeekCompletion::ResumePlayback) => {
                self.resume_video_preview_playback()
            }
            Some(VideoPreviewSeekCompletion::StayPaused) | None => Command::none(),
        }
    }

    pub(super) fn change_video_preview_volume(&mut self, volume: f32) -> Command<Message> {
        let Some(playback) = self.video_preview.as_mut() else {
            return Command::none();
        };
        let volume = volume.clamp(0.0, 1.0);
        playback.volume = volume;
        if let Some(runtime) = playback.audio_runtime.as_ref() {
            runtime.set_volume(volume);
        }
        Command::none()
    }

    pub(super) fn update_video_preview_playback(&mut self) -> Command<Message> {
        let Some(playback) = self.video_preview.as_mut() else {
            return Command::none();
        };
        if playback.status != VideoPreviewPlaybackStatus::Playing {
            return Command::none();
        }

        refresh_video_preview_position(playback);
        if video_preview_reached_end(playback) {
            finish_video_preview_audio(playback);
            playback.status = VideoPreviewPlaybackStatus::Finished;
            playback.seek_completion = None;
            playback.seek_frame_in_flight = None;
            playback.pending_seek_frame = None;
            playback.started_at = None;
        }
        Command::none()
    }

    pub(super) fn video_preview_is_active(&self) -> bool {
        self.video_preview.as_ref().is_some_and(|playback| {
            playback.status == VideoPreviewPlaybackStatus::Playing
                && active_video_preview_path(self.preview.as_ref()) == Some(playback.path.as_path())
        })
    }

    pub(super) fn clear_preview(&mut self) {
        self.clear_audio_preview();
        self.clear_video_preview();
        self.preview = None;
    }

    fn start_audio_preview_playback(&mut self, path: PathBuf) -> Command<Message> {
        self.clear_audio_preview();
        self.audio_preview = Some(AudioPreviewPlayback::loading(path.clone()));
        start_audio_preview_command(path)
    }

    fn clear_audio_preview(&mut self) {
        if let Some(mut playback) = self.audio_preview.take() {
            if let Some(runtime) = playback.runtime.take() {
                runtime.stop();
            }
        }
    }

    fn start_video_preview_playback(
        &mut self,
        path: PathBuf,
        duration: Option<Duration>,
    ) -> Command<Message> {
        self.clear_video_preview();
        let playback = VideoPreviewPlayback::playing(path.clone(), duration);
        let generation = playback.generation;
        self.video_preview = Some(playback);
        Command::batch([
            start_video_preview_audio_command(path.clone(), generation, Duration::ZERO),
            video_preview_frame_command(path.clone(), generation, Duration::ZERO),
            video_preview_metadata_command(path),
        ])
    }

    fn resume_video_preview_playback(&mut self) -> Command<Message> {
        let Some(playback) = self.video_preview.as_mut() else {
            return Command::none();
        };
        let path = playback.path.clone();
        let position = playback.position;
        playback.generation = playback.generation.saturating_add(1);
        playback.status = VideoPreviewPlaybackStatus::Playing;
        playback.seek_completion = None;
        playback.seek_frame_in_flight = None;
        playback.pending_seek_frame = None;
        playback.started_at = Some(Instant::now());
        playback.error = None;
        let generation = playback.generation;

        let should_start_audio = if let Some(runtime) = playback.audio_runtime.as_ref() {
            runtime.set_volume(playback.volume);
            match runtime.seek_to(position) {
                Ok(()) => {
                    runtime.play();
                    false
                }
                Err(error) => {
                    playback.error = Some(error);
                    true
                }
            }
        } else {
            true
        };

        let frame_command = video_preview_frame_command(path.clone(), generation, position);
        if should_start_audio {
            finish_video_preview_audio(playback);
            Command::batch([
                start_video_preview_audio_command(path, generation, position),
                frame_command,
            ])
        } else {
            frame_command
        }
    }

    fn finish_video_seek_frame_decode(
        &mut self,
        path: PathBuf,
        generation: u64,
        position: Duration,
    ) -> Command<Message> {
        let Some(playback) = self
            .video_preview
            .as_mut()
            .filter(|playback| playback.path == path && playback.generation == generation)
        else {
            return Command::none();
        };

        if playback.seek_frame_in_flight != Some(position) {
            return Command::none();
        }
        playback.seek_frame_in_flight = None;
        let Some(next_position) = playback.pending_seek_frame.take() else {
            return Command::none();
        };
        if next_position == position {
            return Command::none();
        }
        start_video_seek_frame_decode(playback, next_position)
    }

    fn clear_video_preview(&mut self) {
        if let Some(mut playback) = self.video_preview.take() {
            finish_video_preview_audio(&mut playback);
        }
    }
}

fn active_audio_preview_path(preview: Option<&PreviewState>) -> Option<&Path> {
    match preview? {
        PreviewState::Loading(path) => Some(path.as_path()),
        PreviewState::Ready(PreviewContent::Audio { path, .. }) => Some(path.as_path()),
        _ => None,
    }
}

fn active_video_preview_path(preview: Option<&PreviewState>) -> Option<&Path> {
    match preview? {
        PreviewState::Ready(PreviewContent::Video { path, .. }) => Some(path.as_path()),
        _ => None,
    }
}

fn refresh_video_preview_position(playback: &mut VideoPreviewPlayback) {
    if playback.status != VideoPreviewPlaybackStatus::Playing {
        return;
    }
    let Some(started_at) = playback.started_at.replace(Instant::now()) else {
        return;
    };
    playback.position =
        clamp_video_preview_position(playback.position + started_at.elapsed(), playback.duration);
}

fn clamp_video_preview_position(position: Duration, duration: Option<Duration>) -> Duration {
    match duration {
        Some(duration) => position.min(duration),
        None => position,
    }
}

fn start_video_seek_frame_decode(
    playback: &mut VideoPreviewPlayback,
    position: Duration,
) -> Command<Message> {
    let position = clamp_video_preview_position(position, playback.duration);
    playback.seek_frame_in_flight = Some(position);
    video_preview_frame_command(playback.path.clone(), playback.generation, position)
}

fn video_preview_reached_end(playback: &VideoPreviewPlayback) -> bool {
    playback
        .duration
        .is_some_and(|duration| playback.position >= duration)
}

fn finish_video_preview_audio(playback: &mut VideoPreviewPlayback) {
    if let Some(runtime) = playback.audio_runtime.take() {
        runtime.stop();
    }
}

fn preview_tree_entries(preview: Option<&PreviewState>) -> Option<&[PreviewTreeEntry]> {
    match preview? {
        PreviewState::Ready(
            PreviewContent::Directory { entries, .. } | PreviewContent::Archive { entries, .. },
        ) => Some(entries),
        _ => None,
    }
}

fn preview_tree_entries_mut(preview: Option<&mut PreviewState>) -> Option<&mut [PreviewTreeEntry]> {
    match preview? {
        PreviewState::Ready(
            PreviewContent::Directory { entries, .. } | PreviewContent::Archive { entries, .. },
        ) => Some(entries),
        _ => None,
    }
}

fn preview_tree_rotation_is_active(entry: &PreviewTreeEntry) -> bool {
    entry.is_directory()
        && (entry.toggle_rotation_progress - preview_tree_rotation_target(entry)).abs()
            > PREVIEW_TREE_TOGGLE_ROTATION_EPSILON
}

fn preview_tree_rotation_target(entry: &PreviewTreeEntry) -> f32 {
    if entry.is_expanded {
        1.0
    } else {
        0.0
    }
}
