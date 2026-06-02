use iced::widget::text_input;
use iced::{event, mouse, window, Command, Size};

use super::{
    rename_input_focus_check_command, FileBrowser, DEFAULT_AUDIO_PREVIEW_HEIGHT,
    DEFAULT_AUDIO_PREVIEW_WIDTH, DEFAULT_IMAGE_PREVIEW_HEIGHT, DEFAULT_IMAGE_PREVIEW_WIDTH,
    DEFAULT_PREVIEW_HEIGHT, DEFAULT_PREVIEW_WIDTH, DEFAULT_SEARCH_HEIGHT, DEFAULT_SEARCH_WIDTH,
    DEFAULT_VIDEO_PREVIEW_HEIGHT, DEFAULT_VIDEO_PREVIEW_WIDTH, MAX_AUDIO_PREVIEW_HEIGHT,
    MAX_AUDIO_PREVIEW_WIDTH, MAX_IMAGE_PREVIEW_HEIGHT, MAX_IMAGE_PREVIEW_WIDTH, MAX_PREVIEW_HEIGHT,
    MAX_PREVIEW_WIDTH, MAX_VIDEO_PREVIEW_HEIGHT, MAX_VIDEO_PREVIEW_WIDTH, MIN_AUDIO_PREVIEW_HEIGHT,
    MIN_AUDIO_PREVIEW_WIDTH, MIN_IMAGE_PREVIEW_HEIGHT, MIN_IMAGE_PREVIEW_WIDTH, MIN_PREVIEW_HEIGHT,
    MIN_PREVIEW_WIDTH, MIN_SEARCH_HEIGHT, MIN_SEARCH_WIDTH, MIN_VIDEO_PREVIEW_HEIGHT,
    MIN_VIDEO_PREVIEW_WIDTH, PREVIEW_WINDOW_APP_ID, SEARCH_WINDOW_APP_ID,
};
use crate::model::{Message, PreviewSize, PreviewWindowProfile};
use crate::view::path_input_id;

const VIDEO_PREVIEW_WINDOW_CONTROL_HEIGHT: f32 = 88.0;
const PREVIEW_RESIZE_MATCH_TOLERANCE: f32 = 1.0;

fn search_window_settings() -> window::Settings {
    let mut settings = window::Settings {
        size: Size::new(DEFAULT_SEARCH_WIDTH, DEFAULT_SEARCH_HEIGHT),
        min_size: Some(Size::new(MIN_SEARCH_WIDTH, MIN_SEARCH_HEIGHT)),
        exit_on_close_request: false,
        ..window::Settings::default()
    };
    settings.platform_specific.application_id = SEARCH_WINDOW_APP_ID.to_owned();
    settings
}

fn preview_window_settings(profile: PreviewWindowProfile, size: PreviewSize) -> window::Settings {
    let size = clamp_preview_size(profile, size);
    let min_size = preview_min_size(profile);
    let max_size = preview_max_size(profile);
    let mut settings = window::Settings {
        size: Size::new(size.width, size.height),
        min_size: Some(min_size),
        max_size: Some(max_size),
        exit_on_close_request: false,
        ..window::Settings::default()
    };
    settings.platform_specific.application_id = PREVIEW_WINDOW_APP_ID.to_owned();
    settings
}

fn default_preview_size(profile: PreviewWindowProfile) -> PreviewSize {
    match profile {
        PreviewWindowProfile::Regular => PreviewSize {
            width: DEFAULT_PREVIEW_WIDTH,
            height: DEFAULT_PREVIEW_HEIGHT,
        },
        PreviewWindowProfile::Image => PreviewSize {
            width: DEFAULT_IMAGE_PREVIEW_WIDTH,
            height: DEFAULT_IMAGE_PREVIEW_HEIGHT,
        },
        PreviewWindowProfile::Audio => PreviewSize {
            width: DEFAULT_AUDIO_PREVIEW_WIDTH,
            height: DEFAULT_AUDIO_PREVIEW_HEIGHT,
        },
        PreviewWindowProfile::Video => PreviewSize {
            width: DEFAULT_VIDEO_PREVIEW_WIDTH,
            height: DEFAULT_VIDEO_PREVIEW_HEIGHT,
        },
    }
}

fn clamp_preview_size(profile: PreviewWindowProfile, size: PreviewSize) -> PreviewSize {
    let min_size = preview_min_size(profile);
    let max_size = preview_max_size(profile);
    PreviewSize {
        width: size.width.clamp(min_size.width, max_size.width),
        height: size.height.clamp(min_size.height, max_size.height),
    }
}

fn preview_min_size(profile: PreviewWindowProfile) -> Size {
    match profile {
        PreviewWindowProfile::Regular => Size::new(MIN_PREVIEW_WIDTH, MIN_PREVIEW_HEIGHT),
        PreviewWindowProfile::Image => Size::new(MIN_IMAGE_PREVIEW_WIDTH, MIN_IMAGE_PREVIEW_HEIGHT),
        PreviewWindowProfile::Audio => Size::new(MIN_AUDIO_PREVIEW_WIDTH, MIN_AUDIO_PREVIEW_HEIGHT),
        PreviewWindowProfile::Video => Size::new(MIN_VIDEO_PREVIEW_WIDTH, MIN_VIDEO_PREVIEW_HEIGHT),
    }
}

fn preview_max_size(profile: PreviewWindowProfile) -> Size {
    match profile {
        PreviewWindowProfile::Regular => Size::new(MAX_PREVIEW_WIDTH, MAX_PREVIEW_HEIGHT),
        PreviewWindowProfile::Image => Size::new(MAX_IMAGE_PREVIEW_WIDTH, MAX_IMAGE_PREVIEW_HEIGHT),
        PreviewWindowProfile::Audio => Size::new(MAX_AUDIO_PREVIEW_WIDTH, MAX_AUDIO_PREVIEW_HEIGHT),
        PreviewWindowProfile::Video => Size::new(MAX_VIDEO_PREVIEW_WIDTH, MAX_VIDEO_PREVIEW_HEIGHT),
    }
}

fn image_preview_size_from_dimensions(width: u32, height: u32) -> PreviewSize {
    let max_size = preview_max_size(PreviewWindowProfile::Image);
    let image_width = width as f32;
    let image_height = height as f32;
    let scale = (max_size.width / image_width)
        .min(max_size.height / image_height)
        .min(1.0);

    PreviewSize {
        width: image_width * scale,
        height: image_height * scale,
    }
}

fn video_preview_size_from_frame(width: u32, height: u32) -> PreviewSize {
    let max_size = preview_max_size(PreviewWindowProfile::Video);
    let max_frame_height = (max_size.height - VIDEO_PREVIEW_WINDOW_CONTROL_HEIGHT).max(1.0);
    let frame_width = width as f32;
    let frame_height = height as f32;
    let scale = (max_size.width / frame_width)
        .min(max_frame_height / frame_height)
        .min(1.0);

    PreviewSize {
        width: frame_width * scale,
        height: frame_height * scale + VIDEO_PREVIEW_WINDOW_CONTROL_HEIGHT,
    }
}

fn preview_size_matches(actual: PreviewSize, expected: PreviewSize) -> bool {
    (actual.width - expected.width).abs() <= PREVIEW_RESIZE_MATCH_TOLERANCE
        && (actual.height - expected.height).abs() <= PREVIEW_RESIZE_MATCH_TOLERANCE
}

impl FileBrowser {
    pub(super) fn window_title(&self, window: window::Id) -> String {
        if self.search_window == Some(window) {
            "Search - File Manager".to_owned()
        } else if self.preview_window == Some(window) {
            "Preview - File Manager".to_owned()
        } else {
            "File Manager".to_owned()
        }
    }

    pub(super) fn ensure_search_window(&mut self) -> Command<Message> {
        if let Some(window) = self.search_window {
            self.focused_window = window;
            return window::gain_focus(window);
        }

        let (window, command) = window::spawn(search_window_settings());
        self.search_window = Some(window);
        self.focused_window = window;
        command
    }

    pub(super) fn close_search_window(&mut self) -> Command<Message> {
        self.search = None;
        let Some(window) = self.search_window.take() else {
            return Command::none();
        };
        if self.focused_window == window {
            self.focused_window = window::Id::MAIN;
        }
        window::close(window)
    }

    pub(super) fn ensure_preview_window(
        &mut self,
        profile: PreviewWindowProfile,
    ) -> Command<Message> {
        self.preview_window_profile = profile;
        self.preview_size = default_preview_size(profile);
        let size = clamp_preview_size(profile, self.preview_size);
        self.pending_preview_resize = Some(size);
        if let Some(window) = self.preview_window {
            self.focused_window = window;
            return Command::batch([
                window::resize(window, Size::new(size.width, size.height)),
                window::gain_focus(window),
            ]);
        }

        let (window, command) = window::spawn(preview_window_settings(profile, self.preview_size));
        self.preview_window = Some(window);
        self.focused_window = window;
        command
    }

    pub(super) fn handle_captured_preview_shortcut(&mut self) -> Command<Message> {
        if self.preview_window == Some(self.focused_window) {
            self.close_preview_window()
        } else {
            Command::none()
        }
    }

    pub(super) fn open_image_preview_window_for_dimensions(
        &mut self,
        width: u32,
        height: u32,
    ) -> Command<Message> {
        if width == 0 || height == 0 {
            return self.open_image_preview_error_window();
        }
        self.recreate_preview_window(
            PreviewWindowProfile::Image,
            image_preview_size_from_dimensions(width, height),
        )
    }

    pub(super) fn open_image_preview_error_window(&mut self) -> Command<Message> {
        self.recreate_preview_window(
            PreviewWindowProfile::Image,
            default_preview_size(PreviewWindowProfile::Image),
        )
    }

    fn recreate_preview_window(
        &mut self,
        profile: PreviewWindowProfile,
        size: PreviewSize,
    ) -> Command<Message> {
        self.preview_window_profile = profile;
        self.preview_size = clamp_preview_size(profile, size);
        self.pending_preview_resize = Some(self.preview_size);

        let close_command = if let Some(window) = self.preview_window.take() {
            if self.focused_window == window {
                self.focused_window = window::Id::MAIN;
            }
            window::close(window)
        } else {
            Command::none()
        };

        let (window, command) = window::spawn(preview_window_settings(profile, self.preview_size));
        self.preview_window = Some(window);
        self.focused_window = window;
        Command::batch([close_command, command])
    }

    pub(super) fn fit_preview_window_to_video_frame(
        &mut self,
        width: u32,
        height: u32,
    ) -> Command<Message> {
        if width == 0 || height == 0 {
            return Command::none();
        }

        self.recreate_preview_window(
            PreviewWindowProfile::Video,
            video_preview_size_from_frame(width, height),
        )
    }

    pub(super) fn close_preview_window(&mut self) -> Command<Message> {
        self.clear_preview();
        self.pending_preview_resize = None;
        let Some(window) = self.preview_window.take() else {
            return Command::none();
        };
        if self.focused_window == window {
            self.focused_window = window::Id::MAIN;
        }
        window::close(window)
    }

    pub(super) fn handle_window_focused(&mut self, window: window::Id) -> Command<Message> {
        self.focused_window = window;
        Command::none()
    }

    pub(super) fn handle_window_unfocused(&mut self, window: window::Id) -> Command<Message> {
        if self.preview_window == Some(window) {
            self.close_preview_window()
        } else {
            Command::none()
        }
    }

    pub(super) fn handle_focused_window_escape_pressed(&mut self) -> Command<Message> {
        if self.search_window == Some(self.focused_window) {
            return self.close_search_window();
        }
        if self.preview_window == Some(self.focused_window) {
            return self.close_preview_window();
        }
        self.dismiss_floating()
    }

    pub(super) fn handle_window_pointer_pressed(
        &mut self,
        button: mouse::Button,
        status: event::Status,
    ) -> Command<Message> {
        if self.preview_window == Some(self.focused_window) {
            return Command::none();
        }

        let pointer_command = match (button, status) {
            (mouse::Button::Left | mouse::Button::Right, event::Status::Captured) => {
                if self.renaming.is_some() {
                    rename_input_focus_check_command()
                } else {
                    Command::none()
                }
            }
            (mouse::Button::Left, event::Status::Ignored) => self.dismiss_floating(),
            _ => Command::none(),
        };

        if self.preview_window.is_some() {
            Command::batch([self.close_preview_window(), pointer_command])
        } else {
            pointer_command
        }
    }

    pub(super) fn dismiss_floating(&mut self) -> Command<Message> {
        if self.transfer_conflict.is_some() {
            self.transfer_conflict = None;
            return Command::none();
        }

        let had_path_suggestions = !self.path_suggestions.is_empty();
        self.context_menu = None;
        self.is_column_view_settings_open = false;
        self.operation_queue.close_panel();
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        let command = self.commit_rename_if_active();
        if had_path_suggestions {
            Command::batch([command, text_input::focus(path_input_id())])
        } else {
            command
        }
    }

    pub(super) fn close_auxiliary_window(&mut self, window_id: window::Id) -> Command<Message> {
        if self.is_shutting_down {
            return Command::none();
        }

        if window_id == window::Id::MAIN {
            return self.close_all_windows();
        }

        if self.search_window == Some(window_id) {
            self.close_search_window()
        } else if self.preview_window == Some(window_id) {
            self.close_preview_window()
        } else {
            Command::none()
        }
    }

    fn close_all_windows(&mut self) -> Command<Message> {
        self.is_shutting_down = true;
        let _ = self.operation_queue.cancel_all();
        self.search = None;
        self.clear_preview();
        self.pending_preview_resize = None;

        let mut commands = Vec::with_capacity(3);
        if let Some(window) = self.search_window.take() {
            commands.push(window::close(window));
        }
        if let Some(window) = self.preview_window.take() {
            commands.push(window::close(window));
        }
        commands.push(window::close(window::Id::MAIN));

        Command::batch(commands)
    }

    pub(super) fn handle_auxiliary_window_resized(
        &mut self,
        window: window::Id,
        width: u32,
        height: u32,
    ) -> Command<Message> {
        if self.preview_window == Some(window) {
            let resized_size = clamp_preview_size(
                self.preview_window_profile,
                PreviewSize {
                    width: width as f32,
                    height: height as f32,
                },
            );
            if let Some(pending_size) = self.pending_preview_resize {
                if preview_size_matches(resized_size, pending_size) {
                    self.pending_preview_resize = None;
                    self.preview_size = resized_size;
                } else {
                    return window::resize(
                        window,
                        Size::new(pending_size.width, pending_size.height),
                    );
                }
            } else {
                self.preview_size = resized_size;
            }
            return self.refresh_preview_thumbnail_for_size();
        }
        Command::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOAT_TOLERANCE: f32 = 0.01;

    fn clamped_image_size(width: u32, height: u32) -> PreviewSize {
        clamp_preview_size(
            PreviewWindowProfile::Image,
            image_preview_size_from_dimensions(width, height),
        )
    }

    fn clamped_video_size(width: u32, height: u32) -> PreviewSize {
        clamp_preview_size(
            PreviewWindowProfile::Video,
            video_preview_size_from_frame(width, height),
        )
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= FLOAT_TOLERANCE,
            "expected {actual} to be within {FLOAT_TOLERANCE} of {expected}"
        );
    }

    #[test]
    fn image_preview_size_fits_large_landscape_to_max_width() {
        let size = clamped_image_size(3_000, 1_000);
        let max_size = preview_max_size(PreviewWindowProfile::Image);

        assert_close(size.width, max_size.width);
        assert!(size.height < max_size.height);
        assert_close(size.width / size.height, 3.0);
    }

    #[test]
    fn image_preview_size_fits_large_portrait_to_max_height() {
        let size = clamped_image_size(1_000, 2_000);
        let max_size = preview_max_size(PreviewWindowProfile::Image);

        assert!(size.width < max_size.width);
        assert_close(size.height, max_size.height);
        assert_close(size.height / size.width, 2.0);
    }

    #[test]
    fn image_preview_size_clamps_small_images_to_minimum_window() {
        let size = clamped_image_size(64, 32);
        let min_size = preview_min_size(PreviewWindowProfile::Image);

        assert_close(size.width, min_size.width);
        assert_close(size.height, min_size.height);
    }

    #[test]
    fn image_preview_size_keeps_medium_landscape_tight() {
        let size = clamped_image_size(748, 499);

        assert_close(size.width, 748.0);
        assert_close(size.height, 499.0);
    }

    #[test]
    fn image_preview_size_keeps_medium_portrait_tight() {
        let size = clamped_image_size(400, 600);

        assert_close(size.width, 400.0);
        assert_close(size.height, 600.0);
    }

    #[test]
    fn video_preview_size_keeps_medium_frame_plus_controls_tight() {
        let size = clamped_video_size(640, 360);

        assert_close(size.width, 640.0);
        assert_close(size.height, 360.0 + VIDEO_PREVIEW_WINDOW_CONTROL_HEIGHT);
    }

    #[test]
    fn video_preview_size_fits_large_portrait_frame_to_max_height() {
        let size = clamped_video_size(720, 1280);
        let max_size = preview_max_size(PreviewWindowProfile::Video);
        let frame_height = size.height - VIDEO_PREVIEW_WINDOW_CONTROL_HEIGHT;

        assert!(size.width < max_size.width);
        assert_close(size.height, max_size.height);
        assert_close(frame_height / size.width, 1280.0 / 720.0);
    }
}
