use iced::advanced::widget as advanced_widget;
use iced::advanced::widget::operation::{Focusable, Operation, Outcome};
use iced::{event, mouse, window, Rectangle, Size, Task};

use super::FileBrowser;
use crate::model::{Message, PreviewSize, PreviewWindowProfile};
use crate::view::{path_input_id, rename_input_id};

const DEFAULT_PREVIEW_WIDTH: f32 = 720.0;
const DEFAULT_PREVIEW_HEIGHT: f32 = 440.0;
const MIN_PREVIEW_WIDTH: f32 = 420.0;
const MIN_PREVIEW_HEIGHT: f32 = 260.0;
const MAX_PREVIEW_WIDTH: f32 = 1080.0;
const MAX_PREVIEW_HEIGHT: f32 = 760.0;
const DEFAULT_IMAGE_PREVIEW_WIDTH: f32 = 748.0;
const DEFAULT_IMAGE_PREVIEW_HEIGHT: f32 = 636.0;
const MIN_IMAGE_PREVIEW_WIDTH: f32 = 360.0;
const MIN_IMAGE_PREVIEW_HEIGHT: f32 = 260.0;
const MAX_IMAGE_PREVIEW_WIDTH: f32 = 1080.0;
const MAX_IMAGE_PREVIEW_HEIGHT: f32 = 940.0;
const DEFAULT_AUDIO_PREVIEW_WIDTH: f32 = 780.0;
const DEFAULT_AUDIO_PREVIEW_HEIGHT: f32 = 168.0;
const MIN_AUDIO_PREVIEW_WIDTH: f32 = 560.0;
const MIN_AUDIO_PREVIEW_HEIGHT: f32 = 136.0;
const MAX_AUDIO_PREVIEW_WIDTH: f32 = 1080.0;
const MAX_AUDIO_PREVIEW_HEIGHT: f32 = 240.0;
const DEFAULT_VIDEO_PREVIEW_WIDTH: f32 = 748.0;
const DEFAULT_VIDEO_PREVIEW_HEIGHT: f32 = 589.0;
const MIN_VIDEO_PREVIEW_WIDTH: f32 = 360.0;
const MIN_VIDEO_PREVIEW_HEIGHT: f32 = 320.0;
const MAX_VIDEO_PREVIEW_WIDTH: f32 = 1080.0;
const MAX_VIDEO_PREVIEW_HEIGHT: f32 = 940.0;
const DEFAULT_SEARCH_WIDTH: f32 = 680.0;
const DEFAULT_SEARCH_HEIGHT: f32 = 460.0;
const MIN_SEARCH_WIDTH: f32 = 520.0;
const MIN_SEARCH_HEIGHT: f32 = 360.0;
const DEFAULT_SETTINGS_WIDTH: f32 = 760.0;
const DEFAULT_SETTINGS_HEIGHT: f32 = 560.0;
const MIN_SETTINGS_WIDTH: f32 = 640.0;
const MIN_SETTINGS_HEIGHT: f32 = 420.0;
const DEFAULT_PROPERTIES_WIDTH: f32 = 760.0;
const DEFAULT_PROPERTIES_HEIGHT: f32 = 560.0;
const MIN_PROPERTIES_WIDTH: f32 = 680.0;
const MIN_PROPERTIES_HEIGHT: f32 = 440.0;
pub(super) const MAIN_WINDOW_INITIAL_WIDTH: f32 = 1180.0;
pub(super) const MAIN_WINDOW_INITIAL_HEIGHT: f32 = 680.0;
const MAIN_WINDOW_APP_ID: &str = "file-manager";
const SEARCH_WINDOW_APP_ID: &str = "file-manager-search";
const SETTINGS_WINDOW_APP_ID: &str = "file-manager-settings";
const PROPERTIES_WINDOW_APP_ID: &str = "file-manager-properties";
const PREVIEW_WINDOW_APP_ID: &str = "file-manager-preview";
const VIDEO_PREVIEW_WINDOW_CONTROL_HEIGHT: f32 = 88.0;
const PREVIEW_RESIZE_MATCH_TOLERANCE: f32 = 1.0;

pub(super) fn main_window_settings() -> window::Settings {
    let mut settings = window::Settings {
        size: Size::new(MAIN_WINDOW_INITIAL_WIDTH, MAIN_WINDOW_INITIAL_HEIGHT),
        exit_on_close_request: false,
        ..window::Settings::default()
    };
    settings.platform_specific.application_id = MAIN_WINDOW_APP_ID.to_owned();
    settings
}

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

fn settings_window_settings() -> window::Settings {
    let mut settings = window::Settings {
        size: Size::new(DEFAULT_SETTINGS_WIDTH, DEFAULT_SETTINGS_HEIGHT),
        min_size: Some(Size::new(MIN_SETTINGS_WIDTH, MIN_SETTINGS_HEIGHT)),
        exit_on_close_request: false,
        ..window::Settings::default()
    };
    settings.platform_specific.application_id = SETTINGS_WINDOW_APP_ID.to_owned();
    settings
}

fn properties_window_settings() -> window::Settings {
    let mut settings = window::Settings {
        size: Size::new(DEFAULT_PROPERTIES_WIDTH, DEFAULT_PROPERTIES_HEIGHT),
        min_size: Some(Size::new(MIN_PROPERTIES_WIDTH, MIN_PROPERTIES_HEIGHT)),
        exit_on_close_request: false,
        ..window::Settings::default()
    };
    settings.platform_specific.application_id = PROPERTIES_WINDOW_APP_ID.to_owned();
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

pub(super) fn default_preview_size(profile: PreviewWindowProfile) -> PreviewSize {
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

struct RenameInputFocusCheck {
    target: advanced_widget::Id,
    is_focused: bool,
}

impl RenameInputFocusCheck {
    fn new(target: iced::widget::Id) -> Self {
        Self {
            target: target.into(),
            is_focused: false,
        }
    }
}

impl Operation<Message> for RenameInputFocusCheck {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Message>)) {
        operate(self);
    }

    fn focusable(
        &mut self,
        id: Option<&advanced_widget::Id>,
        _bounds: Rectangle,
        state: &mut dyn Focusable,
    ) {
        if id == Some(&self.target) {
            self.is_focused = state.is_focused();
        }
    }

    fn finish(&self) -> Outcome<Message> {
        Outcome::Some(Message::RenameInputFocusChecked(self.is_focused))
    }
}

fn rename_input_focus_check_command() -> Task<Message> {
    advanced_widget::operate(RenameInputFocusCheck::new(rename_input_id()))
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
        } else if self.settings_window == Some(window) {
            "Settings - File Manager".to_owned()
        } else if self.properties_window == Some(window) {
            "Properties - File Manager".to_owned()
        } else if self.preview_window == Some(window) {
            "Preview - File Manager".to_owned()
        } else {
            "File Manager".to_owned()
        }
    }

    pub(super) fn ensure_search_window(&mut self) -> Task<Message> {
        if let Some(window) = self.search_window {
            self.focused_window = window;
            return window::gain_focus(window);
        }

        let (window, command) = window::open(search_window_settings());
        self.search_window = Some(window);
        self.focused_window = window;
        command.discard()
    }

    pub(super) fn close_search_window(&mut self) -> Task<Message> {
        self.search = None;
        let Some(window) = self.search_window.take() else {
            return Task::none();
        };
        if self.focused_window == window {
            self.focused_window = self.main_window;
        }
        window::close(window)
    }

    pub(super) fn open_settings(&mut self) -> Task<Message> {
        self.context_menu = None;
        self.shortcut_capture = None;
        self.operation_queue.close_panel();
        self.file_drag = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.selection_marquee = None;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        Task::batch([
            self.commit_rename_if_active(),
            self.ensure_settings_window(),
        ])
    }

    fn ensure_settings_window(&mut self) -> Task<Message> {
        if let Some(window) = self.settings_window {
            self.focused_window = window;
            return window::gain_focus(window);
        }

        let (window, command) = window::open(settings_window_settings());
        self.settings_window = Some(window);
        self.focused_window = window;
        command.discard()
    }

    pub(super) fn close_settings_window(&mut self) -> Task<Message> {
        self.shortcut_capture = None;
        let Some(window) = self.settings_window.take() else {
            return Task::none();
        };
        if self.focused_window == window {
            self.focused_window = self.main_window;
        }
        window::close(window)
    }

    pub(super) fn ensure_properties_window(&mut self) -> Task<Message> {
        if let Some(window) = self.properties_window {
            self.focused_window = window;
            return window::gain_focus(window);
        }

        let (window, command) = window::open(properties_window_settings());
        self.properties_window = Some(window);
        self.focused_window = window;
        command.discard()
    }

    pub(super) fn close_properties_window(&mut self) -> Task<Message> {
        self.properties = None;
        let Some(window) = self.properties_window.take() else {
            return Task::none();
        };
        if self.focused_window == window {
            self.focused_window = self.main_window;
        }
        window::close(window)
    }

    pub(super) fn ensure_preview_window(&mut self, profile: PreviewWindowProfile) -> Task<Message> {
        self.preview_window_profile = profile;
        self.preview_size = default_preview_size(profile);
        let size = clamp_preview_size(profile, self.preview_size);
        self.pending_preview_resize = Some(size);
        if let Some(window) = self.preview_window {
            self.focused_window = window;
            return Task::batch([
                window::resize(window, Size::new(size.width, size.height)),
                window::gain_focus(window),
            ]);
        }

        let (window, command) = window::open(preview_window_settings(profile, self.preview_size));
        self.preview_window = Some(window);
        self.focused_window = window;
        command.discard()
    }

    pub(super) fn handle_captured_preview_shortcut(&mut self) -> Task<Message> {
        if self.preview_window == Some(self.focused_window) {
            self.close_preview_window()
        } else {
            Task::none()
        }
    }

    pub(super) fn open_image_preview_window_for_dimensions(
        &mut self,
        width: u32,
        height: u32,
    ) -> Task<Message> {
        if width == 0 || height == 0 {
            return self.open_image_preview_error_window();
        }
        self.recreate_preview_window(
            PreviewWindowProfile::Image,
            image_preview_size_from_dimensions(width, height),
        )
    }

    pub(super) fn open_image_preview_error_window(&mut self) -> Task<Message> {
        self.recreate_preview_window(
            PreviewWindowProfile::Image,
            default_preview_size(PreviewWindowProfile::Image),
        )
    }

    fn recreate_preview_window(
        &mut self,
        profile: PreviewWindowProfile,
        size: PreviewSize,
    ) -> Task<Message> {
        self.preview_window_profile = profile;
        self.preview_size = clamp_preview_size(profile, size);
        self.pending_preview_resize = Some(self.preview_size);

        let close_command = if let Some(window) = self.preview_window.take() {
            if self.focused_window == window {
                self.focused_window = self.main_window;
            }
            window::close(window)
        } else {
            Task::none()
        };

        let (window, command) = window::open(preview_window_settings(profile, self.preview_size));
        self.preview_window = Some(window);
        self.focused_window = window;
        Task::batch([close_command, command.discard()])
    }

    pub(super) fn fit_preview_window_to_video_frame(
        &mut self,
        width: u32,
        height: u32,
    ) -> Task<Message> {
        if width == 0 || height == 0 {
            return Task::none();
        }

        self.recreate_preview_window(
            PreviewWindowProfile::Video,
            video_preview_size_from_frame(width, height),
        )
    }

    pub(super) fn close_preview_window(&mut self) -> Task<Message> {
        self.clear_preview();
        self.pending_preview_resize = None;
        let Some(window) = self.preview_window.take() else {
            return Task::none();
        };
        if self.focused_window == window {
            self.focused_window = self.main_window;
        }
        window::close(window)
    }

    pub(super) fn handle_window_focused(&mut self, window: window::Id) -> Task<Message> {
        self.focused_window = window;
        Task::none()
    }

    pub(super) fn handle_window_unfocused(&mut self, window: window::Id) -> Task<Message> {
        if self.preview_window == Some(window) {
            self.close_preview_window()
        } else {
            Task::none()
        }
    }

    pub(super) fn handle_focused_window_escape_pressed(&mut self) -> Task<Message> {
        if self.search_window == Some(self.focused_window) {
            return self.close_search_window();
        }
        if self.settings_window == Some(self.focused_window) {
            return self.close_settings_window();
        }
        if self.properties_window == Some(self.focused_window) {
            return self.close_properties_window();
        }
        if self.preview_window == Some(self.focused_window) {
            return self.close_preview_window();
        }
        self.dismiss_floating()
    }

    pub(super) fn handle_window_pointer_pressed(
        &mut self,
        window: window::Id,
        button: mouse::Button,
        status: event::Status,
    ) -> Task<Message> {
        if window != self.main_window {
            return Task::none();
        }

        if self.preview_window == Some(self.focused_window) {
            return Task::none();
        }

        if button == mouse::Button::Left && self.start_ctrl_shift_pane_drag(status) {
            return Task::none();
        }

        let pointer_command = match (button, status) {
            (mouse::Button::Left | mouse::Button::Right, event::Status::Captured) => {
                if self.renaming.is_some() {
                    rename_input_focus_check_command()
                } else {
                    Task::none()
                }
            }
            (mouse::Button::Left, event::Status::Ignored) => self.dismiss_floating(),
            _ => Task::none(),
        };

        if self.preview_window.is_some() {
            Task::batch([self.close_preview_window(), pointer_command])
        } else {
            pointer_command
        }
    }

    pub(super) fn dismiss_floating(&mut self) -> Task<Message> {
        if self.destructive_action_confirmation.is_some() {
            self.destructive_action_confirmation = None;
            return Task::none();
        }

        if self.transfer_conflict.is_some() {
            self.transfer_conflict = None;
            return Task::none();
        }

        let had_path_suggestions = !self.path_suggestions.is_empty();
        self.context_menu = None;
        self.shortcut_capture = None;
        self.operation_queue.close_panel();
        self.file_drag = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.selection_marquee = None;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        let command = self.commit_rename_if_active();
        if had_path_suggestions {
            Task::batch([
                command,
                iced::widget::operation::focus(path_input_id(self.active_pane_id())),
            ])
        } else {
            command
        }
    }

    pub(super) fn close_auxiliary_window(&mut self, window_id: window::Id) -> Task<Message> {
        if self.is_shutting_down {
            return Task::none();
        }

        if window_id == self.main_window {
            return self.close_all_windows();
        }

        if self.search_window == Some(window_id) {
            self.close_search_window()
        } else if self.settings_window == Some(window_id) {
            self.close_settings_window()
        } else if self.properties_window == Some(window_id) {
            self.close_properties_window()
        } else if self.preview_window == Some(window_id) {
            self.close_preview_window()
        } else {
            Task::none()
        }
    }

    fn close_all_windows(&mut self) -> Task<Message> {
        self.is_shutting_down = true;
        let _ = self.operation_queue.cancel_all();
        self.search = None;
        self.properties = None;
        self.clear_preview();
        self.pending_preview_resize = None;

        let mut commands = Vec::with_capacity(6);
        if let Some(window) = self.search_window.take() {
            commands.push(window::close(window));
        }
        if let Some(window) = self.settings_window.take() {
            commands.push(window::close(window));
        }
        if let Some(window) = self.properties_window.take() {
            commands.push(window::close(window));
        }
        if let Some(window) = self.preview_window.take() {
            commands.push(window::close(window));
        }
        commands.push(window::close(self.main_window));
        // Iced daemons keep running without windows unless update returns an exit task.
        commands.push(iced::exit());

        Task::batch(commands)
    }

    pub(super) fn handle_auxiliary_window_resized(
        &mut self,
        window: window::Id,
        width: f32,
        height: f32,
    ) -> Task<Message> {
        if window == self.main_window {
            self.main_window_width = width.max(1.0);
            self.main_window_height = height.max(1.0);
            return Task::none();
        }

        if self.preview_window == Some(window) {
            let resized_size =
                clamp_preview_size(self.preview_window_profile, PreviewSize { width, height });
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
        Task::none()
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
