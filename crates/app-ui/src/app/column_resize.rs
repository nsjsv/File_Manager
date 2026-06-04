use iced::{Command, Point};

use crate::app::FileBrowser;
use crate::commands::save_user_config_command;
use crate::config;
use crate::model::{ColumnViewMode, Message};

#[derive(Debug, Clone, Copy)]
pub(super) struct ColumnResizeDrag {
    pub(super) cursor_start_x: f32,
    pub(super) width_start: f32,
}

impl FileBrowser {
    pub(super) fn start_column_resize_drag(&mut self) -> Command<Message> {
        if self.renaming.is_some() {
            return self.commit_rename_if_active();
        }

        if self.column_view_mode != ColumnViewMode::Unbounded {
            return Command::none();
        }

        self.column_resize_drag = Some(ColumnResizeDrag {
            cursor_start_x: self.cursor_position.x,
            width_start: self.unbounded_column_width,
        });
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.clear_preview();
        self.context_menu = None;
        self.is_column_view_settings_open = false;
        Command::none()
    }

    pub(super) fn update_column_resize_drag(&mut self, position: Point) {
        let Some(drag) = self.column_resize_drag else {
            return;
        };

        self.unbounded_column_width = config::normalize_unbounded_column_width(
            drag.width_start + position.x - drag.cursor_start_x,
        );
    }

    pub(super) fn finish_column_resize_drag(&mut self) -> bool {
        let was_resizing = self.column_resize_drag.take().is_some();
        if was_resizing {
            self.user_config.unbounded_column_width = self.unbounded_column_width;
        }
        was_resizing
    }

    pub(super) fn finish_column_resize_drag_command(&mut self) -> Command<Message> {
        if self.finish_column_resize_drag() {
            self.persist_user_config_command()
        } else {
            Command::none()
        }
    }

    pub(super) fn persist_user_config_command(&self) -> Command<Message> {
        save_user_config_command(self.user_config.clone())
    }

    pub(super) fn toggle_show_hidden_files(&mut self) -> Command<Message> {
        self.options.include_hidden = !self.options.include_hidden;
        self.user_config.show_hidden_files = self.options.include_hidden;
        let persist_command = self.persist_user_config_command();
        let reload_command = self.reload_current();
        Command::batch([persist_command, reload_command])
    }
}
