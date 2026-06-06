use iced::{Point, Task};

use crate::app::FileBrowser;
use crate::commands::save_user_config_command;
use crate::config;
use crate::model::Message;
use crate::sidebar::SIDEBAR_WIDTH;
use crate::three_column_view::{COLUMN_RESIZE_DIVIDER_WIDTH, DEFAULT_VISIBLE_COLUMN_COUNT};

#[derive(Debug, Clone, Copy)]
pub(super) struct ColumnResizeDrag {
    pub(super) column_index: usize,
    pub(super) cursor_start_x: f32,
    pub(super) width_start: f32,
}

impl FileBrowser {
    pub(super) fn start_column_resize_drag(&mut self, column_index: usize) -> Task<Message> {
        if self.renaming.is_some() {
            return self.commit_rename_if_active();
        }

        self.column_resize_drag = Some(ColumnResizeDrag {
            column_index,
            cursor_start_x: self.cursor_position.x,
            width_start: self.column_width(column_index),
        });
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.clear_preview();
        self.context_menu = None;
        self.is_column_view_settings_open = false;
        Task::none()
    }

    pub(super) fn update_column_resize_drag(&mut self, position: Point) {
        let Some(drag) = self.column_resize_drag else {
            return;
        };

        self.column_width_overrides.insert(
            drag.column_index,
            config::normalize_column_width(drag.width_start + position.x - drag.cursor_start_x),
        );
    }

    pub(super) fn finish_column_resize_drag(&mut self) -> bool {
        let was_resizing = self.column_resize_drag.take().is_some();
        if was_resizing {
            self.user_config.column_width_overrides = self.column_width_overrides.clone();
        }
        was_resizing
    }

    pub(crate) fn column_width(&self, column_index: usize) -> f32 {
        self.column_width_overrides
            .get(&column_index)
            .copied()
            .unwrap_or_else(|| self.default_column_width())
    }

    fn default_column_width(&self) -> f32 {
        let divider_width = COLUMN_RESIZE_DIVIDER_WIDTH * (DEFAULT_VISIBLE_COLUMN_COUNT - 1) as f32;
        let content_width = (self.main_window_width - SIDEBAR_WIDTH - divider_width).max(1.0);
        let width = content_width / DEFAULT_VISIBLE_COLUMN_COUNT as f32;
        if width.is_finite() {
            width.max(config::MIN_COLUMN_WIDTH)
        } else {
            config::MIN_COLUMN_WIDTH
        }
    }

    pub(super) fn finish_column_resize_drag_command(&mut self) -> Task<Message> {
        if self.finish_column_resize_drag() {
            self.persist_user_config_command()
        } else {
            Task::none()
        }
    }

    pub(super) fn persist_user_config_command(&self) -> Task<Message> {
        save_user_config_command(self.user_config.clone())
    }

    pub(super) fn toggle_show_hidden_files(&mut self) -> Task<Message> {
        self.options.include_hidden = !self.options.include_hidden;
        self.user_config.show_hidden_files = self.options.include_hidden;
        let persist_command = self.persist_user_config_command();
        let reload_command = self.reload_current();
        Task::batch([persist_command, reload_command])
    }
}
