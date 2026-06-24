use super::FileBrowser;
use iced::Task;

use crate::model::Message;

impl FileBrowser {
    pub(super) fn clear_pointer_driven_interaction_state(&mut self) {
        self.tab_drag = None;
        self.pane_drag = None;
        self.file_drag = None;
        self.selection_marquee = None;
        self.drag_selection_anchor = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.sidebar_resize_drag = None;
        self.column_resize_drag = None;
    }

    pub(super) fn finish_pointer_drag_interactions(&mut self) -> Task<Message> {
        self.finish_tab_drag();
        self.finish_pane_drag();
        self.finish_batch_rename_preview_drag();
        Task::batch([
            self.finish_startup_index_entry_selection_drag(),
            self.finish_sidebar_bookmark_drag(),
            self.finish_sidebar_resize_drag_command(),
            self.finish_column_resize_drag_command(),
            self.finish_drag_selection(None),
            self.schedule_thumbnail_refresh(),
        ])
    }
}
