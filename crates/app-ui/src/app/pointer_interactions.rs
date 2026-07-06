use super::FileBrowser;
use iced::{window, Point, Task};

use crate::model::Message;

impl FileBrowser {
    pub(super) fn clear_pointer_driven_interaction_state(&mut self) {
        self.tab_drag = None;
        self.pane_drag = None;
        self.pane_drag_pointer_press = None;
        self.file_drag = None;
        self.selection_marquee = None;
        self.drag_selection_anchor = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.sidebar_resize_drag = None;
        self.column_resize_drag = None;
        self.list_column_resize_drag = None;
        self.list_column_reorder_drag = None;
    }

    pub(super) fn finish_pointer_drag_interactions(&mut self) -> Task<Message> {
        self.finish_tab_drag();
        self.finish_pane_drag();
        self.finish_batch_rename_preview_drag();
        Task::batch([
            self.finish_sidebar_bookmark_drag(),
            self.finish_sidebar_resize_drag_command(),
            self.finish_column_resize_drag_command(),
            self.finish_list_column_resize_drag_command(),
            self.finish_list_column_reorder_drag_command(),
            self.finish_drag_selection(None),
            self.schedule_thumbnail_refresh(),
        ])
    }

    pub(super) fn update_pointer_motion(
        &mut self,
        window: window::Id,
        position: Point,
    ) -> Task<Message> {
        if window != self.main_window {
            return Task::none();
        }
        self.cursor_position = position;
        self.promote_ctrl_shift_pane_drag_from_active_pointer_drag();
        self.update_tab_drag(position);
        self.update_pane_drag(position);
        let cursor_inside_main_window = (0.0..=self.main_window_width).contains(&position.x)
            && (0.0..=self.main_window_height).contains(&position.y);
        let file_drag_command = if cursor_inside_main_window {
            self.update_file_drag(position)
        } else {
            self.request_file_drag_wayland_dnd_on_window_exit()
        };
        self.update_sidebar_bookmark_drag(position);
        self.update_sidebar_resize_drag(position);
        self.update_column_resize_drag(position);
        self.update_list_column_resize_drag(position);
        self.update_list_column_reorder_drag(position);
        let selection_command = if self.update_selection_marquee(position) {
            crate::column_entry_bounds::column_entry_bounds_command()
        } else {
            Task::none()
        };
        Task::batch([file_drag_command, selection_command])
    }
}
