use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::Command;

use super::FileBrowser;
use crate::commands::start_audio_preview_command;
use crate::model::{
    AudioPreviewPlayback, AudioPreviewPlaybackStatus, Message, PreviewContent, PreviewState,
    PreviewTreeEntry, VideoPreviewFrame,
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
                if !matches!(preview, PreviewContent::Audio { .. }) {
                    self.clear_audio_preview();
                }
                self.preview = Some(PreviewState::Ready(preview));
                self.error = None;
            }
            Err(error) => {
                self.clear_audio_preview();
                self.preview = Some(PreviewState::Error(error));
            }
        }

        Command::none()
    }

    pub(super) fn accept_video_preview_frame(
        &mut self,
        frame: VideoPreviewFrame,
    ) -> Command<Message> {
        let Some(PreviewState::Ready(PreviewContent::Video {
            path,
            frame: current_frame,
            width,
            height,
        })) = &mut self.preview
        else {
            return Command::none();
        };
        if path != &frame.path {
            return Command::none();
        }

        *current_frame = Some(frame.handle);
        *width = frame.width;
        *height = frame.height;
        Command::none()
    }

    pub(super) fn accept_video_preview_error(
        &mut self,
        path: PathBuf,
        error: String,
    ) -> Command<Message> {
        if self.active_video_preview_path().as_ref() == Some(&path) {
            self.preview = Some(PreviewState::Error(error));
        }
        Command::none()
    }

    pub(super) fn active_video_preview_path(&self) -> Option<PathBuf> {
        match &self.preview {
            Some(PreviewState::Ready(PreviewContent::Video { path, .. })) => Some(path.clone()),
            _ => None,
        }
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

    pub(super) fn stop_audio_preview_playback(&mut self) -> Command<Message> {
        let Some(playback) = self.audio_preview.as_mut() else {
            return Command::none();
        };
        if let Some(runtime) = playback.runtime.take() {
            runtime.stop();
        }
        playback.position = std::time::Duration::ZERO;
        playback.status = AudioPreviewPlaybackStatus::Stopped;
        playback.error = None;
        Command::none()
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

    pub(super) fn clear_preview(&mut self) {
        self.clear_audio_preview();
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
}

fn active_audio_preview_path(preview: Option<&PreviewState>) -> Option<&Path> {
    match preview? {
        PreviewState::Loading(path) => Some(path.as_path()),
        PreviewState::Ready(PreviewContent::Audio { path, .. }) => Some(path.as_path()),
        _ => None,
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
