use iced::advanced::widget as advanced_widget;
use iced::advanced::widget::operation::{Focusable, Operation, Outcome};
use iced::{event, mouse, window, Rectangle, Size, Task};

use super::FileBrowser;
use crate::model::{Message, PreviewSize, PreviewWindowProfile, SettingsCategory};
use crate::view::{address_input_id, rename_input_id};

const DEFAULT_PREVIEW_WIDTH: f32 = 720.0;
const DEFAULT_PREVIEW_HEIGHT: f32 = 440.0;
const MIN_PREVIEW_WIDTH: f32 = 420.0;
const MIN_PREVIEW_HEIGHT: f32 = 260.0;
const DEFAULT_IMAGE_PREVIEW_WIDTH: f32 = 748.0;
const DEFAULT_IMAGE_PREVIEW_HEIGHT: f32 = 636.0;
const MIN_IMAGE_PREVIEW_WIDTH: f32 = 360.0;
const MIN_IMAGE_PREVIEW_HEIGHT: f32 = 260.0;
const IMAGE_PREVIEW_INITIAL_FIT_MAX_WIDTH: f32 = 1080.0;
const IMAGE_PREVIEW_INITIAL_FIT_MAX_HEIGHT: f32 = 940.0;
const DEFAULT_AUDIO_PREVIEW_WIDTH: f32 = 780.0;
const DEFAULT_AUDIO_PREVIEW_HEIGHT: f32 = 168.0;
const MIN_AUDIO_PREVIEW_WIDTH: f32 = 560.0;
const MIN_AUDIO_PREVIEW_HEIGHT: f32 = 136.0;
const DEFAULT_VIDEO_PREVIEW_WIDTH: f32 = 748.0;
const DEFAULT_VIDEO_PREVIEW_HEIGHT: f32 = 589.0;
const MIN_VIDEO_PREVIEW_WIDTH: f32 = 360.0;
const MIN_VIDEO_PREVIEW_HEIGHT: f32 = 320.0;
const VIDEO_PREVIEW_INITIAL_FIT_MAX_WIDTH: f32 = 1080.0;
const VIDEO_PREVIEW_INITIAL_FIT_MAX_HEIGHT: f32 = 940.0;
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
    let size = clamp_preview_size_to_minimum(profile, size);
    let min_size = preview_min_size(profile);
    let mut settings = window::Settings {
        size: Size::new(size.width, size.height),
        min_size: Some(min_size),
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

enum TextInputFocusCheckResult {
    Address(crate::model::BrowserPaneId),
    Rename,
}

struct TextInputFocusCheck {
    target: advanced_widget::Id,
    is_focused: bool,
    result: TextInputFocusCheckResult,
}

impl TextInputFocusCheck {
    fn new(target: iced::widget::Id, result: TextInputFocusCheckResult) -> Self {
        Self {
            target: target.into(),
            is_focused: false,
            result,
        }
    }
}

impl Operation<Message> for TextInputFocusCheck {
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
        let message = match self.result {
            TextInputFocusCheckResult::Address(pane_id) => {
                Message::AddressInputFocusChecked(pane_id, self.is_focused)
            }
            TextInputFocusCheckResult::Rename => Message::RenameInputFocusChecked(self.is_focused),
        };
        Outcome::Some(message)
    }
}

fn rename_input_focus_check_command() -> Task<Message> {
    advanced_widget::operate(TextInputFocusCheck::new(
        rename_input_id(),
        TextInputFocusCheckResult::Rename,
    ))
}

fn address_input_focus_check_command(pane_id: crate::model::BrowserPaneId) -> Task<Message> {
    advanced_widget::operate(TextInputFocusCheck::new(
        address_input_id(pane_id),
        TextInputFocusCheckResult::Address(pane_id),
    ))
}

fn clamp_preview_size_to_minimum(profile: PreviewWindowProfile, size: PreviewSize) -> PreviewSize {
    let min_size = preview_min_size(profile);
    PreviewSize {
        width: size.width.max(min_size.width),
        height: size.height.max(min_size.height),
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

fn image_preview_initial_fit_max_size() -> Size {
    Size::new(
        IMAGE_PREVIEW_INITIAL_FIT_MAX_WIDTH,
        IMAGE_PREVIEW_INITIAL_FIT_MAX_HEIGHT,
    )
}

fn video_preview_initial_fit_max_size() -> Size {
    Size::new(
        VIDEO_PREVIEW_INITIAL_FIT_MAX_WIDTH,
        VIDEO_PREVIEW_INITIAL_FIT_MAX_HEIGHT,
    )
}

fn image_preview_size_from_dimensions(width: u32, height: u32) -> PreviewSize {
    let max_size = image_preview_initial_fit_max_size();
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
    let max_size = video_preview_initial_fit_max_size();
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
        if self.settings_window == Some(window) {
            crate::localization::translate_current("Settings - File Manager")
        } else if self.properties_window == Some(window) {
            crate::localization::translate_current("Properties - File Manager")
        } else if self.preview_window == Some(window) {
            crate::localization::translate_current("Preview - File Manager")
        } else if self.search.is_active() {
            crate::localization::translate_current("Search - File Manager")
        } else {
            crate::localization::translate_current("File Manager")
        }
    }

    pub(super) fn open_settings(&mut self) -> Task<Message> {
        self.context_menu = None;
        self.open_with = None;
        self.archive_creation = None;
        self.archive_extraction = None;
        self.shortcut_capture = None;
        self.operation_queue.close_panel();
        self.file_drag = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.selection_marquee = None;
        let _ = self.cancel_address_editing();
        let refresh_application_logs = if self.selected_settings_category == SettingsCategory::Logs
        {
            self.refresh_application_logs()
        } else {
            Task::none()
        };
        Task::batch([
            self.commit_rename_if_active(),
            self.ensure_settings_window(),
            refresh_application_logs,
        ])
    }

    pub(super) fn select_settings_category(&mut self, category: SettingsCategory) -> Task<Message> {
        self.shortcut_capture = None;
        self.search.cancel_force_restart_confirmation();
        self.selected_settings_category = category;
        if category == SettingsCategory::Logs {
            self.refresh_application_logs()
        } else {
            Task::none()
        }
    }

    pub(super) fn ensure_settings_window(&mut self) -> Task<Message> {
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
        self.search.cancel_force_restart_confirmation();
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
        self.clear_file_properties_state();
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
        let size = clamp_preview_size_to_minimum(profile, self.preview_size);
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
        self.preview_size = clamp_preview_size_to_minimum(profile, size);
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
        if self.settings_window == Some(self.focused_window) {
            if self.search.cancel_force_restart_confirmation() {
                return Task::none();
            }
            return self.close_settings_window();
        }
        if self.properties_window == Some(self.focused_window) {
            return self.close_properties_window();
        }
        if self.preview_window == Some(self.focused_window) {
            return self.close_preview_window();
        }
        if self.address_editing.is_some() {
            return self.cancel_address_editing();
        }
        self.dismiss_floating()
    }

    pub(super) fn handle_window_pointer_pressed(
        &mut self,
        window: window::Id,
        button: mouse::Button,
        status: event::Status,
    ) -> Task<Message> {
        if self.settings_window == Some(window) {
            return Task::none();
        }

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
                let mut input_focus_checks = Vec::with_capacity(2);
                if self.renaming.is_some() {
                    input_focus_checks.push(rename_input_focus_check_command());
                }
                if let Some(session) = &self.address_editing {
                    input_focus_checks.push(address_input_focus_check_command(session.pane_id));
                }
                Task::batch(input_focus_checks)
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

        if self.file_drop_prompt.is_some() {
            self.file_drop_prompt = None;
            return Task::none();
        }

        if self.transfer_conflict.is_some() {
            self.transfer_conflict = None;
            return Task::none();
        }

        if self.archive_creation.is_some() {
            self.archive_creation = None;
            return Task::none();
        }

        if self.archive_extraction.is_some() {
            self.archive_extraction = None;
            return Task::none();
        }

        if self.batch_rename.is_some() {
            self.batch_rename = None;
            return Task::none();
        }

        if self.network_connection_editor.is_some() {
            self.network_connection_editor = None;
            return Task::none();
        }

        if self.open_with.is_some() {
            self.open_with = None;
            return Task::none();
        }

        self.context_menu = None;
        self.shortcut_capture = None;
        self.operation_queue.close_panel();
        self.file_drag = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.selection_marquee = None;
        Task::batch([
            self.cancel_address_editing(),
            self.commit_rename_if_active(),
        ])
    }

    pub(super) fn close_auxiliary_window(&mut self, window_id: window::Id) -> Task<Message> {
        if self.is_shutting_down {
            return Task::none();
        }

        if window_id == self.main_window {
            return self.close_all_windows();
        }

        if self.settings_window == Some(window_id) {
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
        self.search.abandon_and_clear_input();
        let _ = self.operation_queue.cancel_all();
        self.clear_file_properties_state();
        self.archive_creation = None;
        self.archive_extraction = None;
        self.clear_preview();
        self.pending_preview_resize = None;

        let mut auxiliary_close_commands = Vec::with_capacity(3);
        if let Some(window) = self.settings_window.take() {
            auxiliary_close_commands.push(window::close(window));
        }
        if let Some(window) = self.properties_window.take() {
            auxiliary_close_commands.push(window::close(window));
        }
        if let Some(window) = self.preview_window.take() {
            auxiliary_close_commands.push(window::close(window));
        }

        let close_main_window_and_exit = Task::batch([
            window::close(self.main_window),
            // Iced daemon 没有窗口后仍会运行，主窗口关闭时必须显式退出运行时。
            iced::exit(),
        ]);
        Task::batch(auxiliary_close_commands).chain(shutdown_after_browser_session_save(
            self.flush_browser_session_save(),
            close_main_window_and_exit,
        ))
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
            return self.request_breadcrumb_drop_target_bounds_measurement();
        }

        if self.preview_window == Some(window) {
            let resized_size = clamp_preview_size_to_minimum(
                self.preview_window_profile,
                PreviewSize { width, height },
            );
            if let Some(pending_size) = self.pending_preview_resize {
                if preview_size_matches(resized_size, pending_size) {
                    self.pending_preview_resize = None;
                    self.preview_size = resized_size;
                } else {
                    tracing::debug!(
                        target: "app_ui::preview",
                        window = ?window,
                        actual_width = resized_size.width,
                        actual_height = resized_size.height,
                        pending_width = pending_size.width,
                        pending_height = pending_size.height,
                        "preview resize still pending"
                    );
                    return window::resize(
                        window,
                        Size::new(pending_size.width, pending_size.height),
                    );
                }
            } else {
                self.preview_size = resized_size;
            }
            tracing::debug!(
                target: "app_ui::preview",
                window = ?window,
                width = self.preview_size.width,
                height = self.preview_size.height,
                "preview window resized"
            );
            return self.refresh_preview_thumbnail_for_size();
        }
        Task::none()
    }
}

fn shutdown_after_browser_session_save(
    browser_session_save: Task<Message>,
    close_main_window_and_exit: Task<Message>,
) -> Task<Message> {
    browser_session_save.chain(close_main_window_and_exit)
}

#[cfg(test)]
mod tests;
