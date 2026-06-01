use iced::widget::text_input;
use iced::{event, mouse, window, Command, Size};

use super::{
    rename_input_focus_check_command, FileBrowser, DEFAULT_AUDIO_PREVIEW_HEIGHT,
    DEFAULT_AUDIO_PREVIEW_WIDTH, DEFAULT_PREVIEW_HEIGHT, DEFAULT_PREVIEW_WIDTH,
    DEFAULT_SEARCH_HEIGHT, DEFAULT_SEARCH_WIDTH, MAX_AUDIO_PREVIEW_HEIGHT, MAX_AUDIO_PREVIEW_WIDTH,
    MAX_PREVIEW_HEIGHT, MAX_PREVIEW_WIDTH, MIN_AUDIO_PREVIEW_HEIGHT, MIN_AUDIO_PREVIEW_WIDTH,
    MIN_PREVIEW_HEIGHT, MIN_PREVIEW_WIDTH, MIN_SEARCH_HEIGHT, MIN_SEARCH_WIDTH,
    PREVIEW_WINDOW_APP_ID, SEARCH_WINDOW_APP_ID,
};
use crate::model::{Message, PreviewSize, PreviewWindowProfile};
use crate::view::path_input_id;

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
        PreviewWindowProfile::Audio => PreviewSize {
            width: DEFAULT_AUDIO_PREVIEW_WIDTH,
            height: DEFAULT_AUDIO_PREVIEW_HEIGHT,
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
        PreviewWindowProfile::Audio => Size::new(MIN_AUDIO_PREVIEW_WIDTH, MIN_AUDIO_PREVIEW_HEIGHT),
    }
}

fn preview_max_size(profile: PreviewWindowProfile) -> Size {
    match profile {
        PreviewWindowProfile::Regular => Size::new(MAX_PREVIEW_WIDTH, MAX_PREVIEW_HEIGHT),
        PreviewWindowProfile::Audio => Size::new(MAX_AUDIO_PREVIEW_WIDTH, MAX_AUDIO_PREVIEW_HEIGHT),
    }
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
        if let Some(window) = self.preview_window {
            self.focused_window = window;
            let size = clamp_preview_size(profile, self.preview_size);
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

    pub(super) fn close_preview_window(&mut self) -> Command<Message> {
        self.clear_preview();
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
            self.preview_size = clamp_preview_size(
                self.preview_window_profile,
                PreviewSize {
                    width: width as f32,
                    height: height as f32,
                },
            );
            return self.refresh_preview_thumbnail_for_size();
        }
        Command::none()
    }
}
