use iced::{Point, Task};

use super::{FileBrowser, POINTER_DRAG_ACTIVATION_DISTANCE};
use crate::model::{
    BrowserPaneId, ContextMenuState, FileDragPhase, ListColumnKind, ListColumnMenuState, Message,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct ListColumnResizeDrag {
    pub(super) kind: ListColumnKind,
    pub(super) cursor_start_x: f32,
    pub(super) width_start: f32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ListColumnReorderDrag {
    kind: ListColumnKind,
    phase: FileDragPhase,
    order_changed: bool,
}

impl ListColumnReorderDrag {
    fn is_dragging(self) -> bool {
        matches!(self.phase, FileDragPhase::Dragging)
    }
}

impl FileBrowser {
    #[cfg(test)]
    pub(super) fn select_list_sort_column(&mut self, column: ListColumnKind) -> Task<Message> {
        self.user_config
            .list_view_preferences
            .select_sort_column(column);
        let sort = self.user_config.list_view_preferences.sort();
        self.options.sort_field = sort.field;
        self.options.sort_direction = sort.direction;
        Task::batch([
            self.persist_user_preferences_command(),
            self.reload_visible_panes(),
        ])
    }

    pub(super) fn open_list_column_menu(&mut self, pane_id: BrowserPaneId) -> Task<Message> {
        self.activate_pane(pane_id);
        let rename_command = self.commit_rename_if_active();
        self.clear_preview();
        self.operation_queue.close_panel();
        self.open_with = None;
        self.file_drag = None;
        self.selection_marquee = None;
        self.drag_selection_anchor = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.path_suggestions.clear();
        self.path_suggestion_selection = None;
        self.context_menu = Some(ContextMenuState::ListColumns(ListColumnMenuState {
            position: self.cursor_position,
        }));
        rename_command
    }

    pub(super) fn toggle_list_column_visibility(
        &mut self,
        column: ListColumnKind,
    ) -> Task<Message> {
        let before_sort = self.user_config.list_view_preferences.sort();
        let visible = self
            .user_config
            .list_view_preferences
            .columns()
            .iter()
            .find(|config| config.kind == column)
            .map_or(true, |config| !config.visible);
        self.user_config
            .list_view_preferences
            .set_column_visible(column, visible);
        let after_sort = self.user_config.list_view_preferences.sort();
        self.options.sort_field = after_sort.field;
        self.options.sort_direction = after_sort.direction;
        if before_sort == after_sort {
            self.persist_user_preferences_command()
        } else {
            Task::batch([
                self.persist_user_preferences_command(),
                self.reload_visible_panes(),
            ])
        }
    }

    pub(super) fn start_list_column_resize_drag(
        &mut self,
        pane_id: BrowserPaneId,
        column: ListColumnKind,
    ) -> Task<Message> {
        self.activate_pane(pane_id);
        if self.renaming.is_some() {
            return self.commit_rename_if_active();
        }

        let width_start = self
            .user_config
            .list_view_preferences
            .columns()
            .iter()
            .find(|config| config.kind == column)
            .map(|config| config.width)
            .unwrap_or(0.0);
        self.list_column_resize_drag = Some(ListColumnResizeDrag {
            kind: column,
            cursor_start_x: self.cursor_position.x,
            width_start,
        });
        self.list_column_reorder_drag = None;
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.clear_preview();
        self.context_menu = None;
        Task::none()
    }

    pub(super) fn start_list_column_reorder_drag(
        &mut self,
        pane_id: BrowserPaneId,
        column: ListColumnKind,
    ) -> Task<Message> {
        self.activate_pane(pane_id);
        if self.renaming.is_some() {
            return self.commit_rename_if_active();
        }

        self.list_column_reorder_drag = Some(ListColumnReorderDrag {
            kind: column,
            phase: FileDragPhase::WaitingForMovement {
                origin: self.cursor_position,
            },
            order_changed: false,
        });
        self.list_column_resize_drag = None;
        self.file_drag = None;
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.sidebar_bookmark_drag = None;
        self.sidebar_bookmark_drop_slot = None;
        self.clear_preview();
        self.context_menu = None;
        Task::none()
    }

    pub(super) fn update_list_column_reorder_drag(&mut self, position: Point) {
        let Some(drag) = &mut self.list_column_reorder_drag else {
            return;
        };
        let FileDragPhase::WaitingForMovement { origin } = drag.phase else {
            return;
        };
        let delta_x = position.x - origin.x;
        let delta_y = position.y - origin.y;
        if delta_x * delta_x + delta_y * delta_y
            >= POINTER_DRAG_ACTIVATION_DISTANCE * POINTER_DRAG_ACTIVATION_DISTANCE
        {
            drag.phase = FileDragPhase::Dragging;
        }
    }

    pub(super) fn enter_list_column_reorder_target(
        &mut self,
        target: ListColumnKind,
    ) -> Task<Message> {
        let Some(drag) = self.list_column_reorder_drag.as_ref() else {
            return Task::none();
        };
        if !drag.is_dragging() {
            return Task::none();
        }
        let dragged = drag.kind;
        let order_changed = self
            .user_config
            .list_view_preferences
            .move_column_to(dragged, target);
        if order_changed {
            if let Some(drag) = &mut self.list_column_reorder_drag {
                drag.order_changed = true;
            }
        }
        Task::none()
    }

    pub(super) fn finish_list_column_reorder_drag_command(&mut self) -> Task<Message> {
        let Some(drag) = self.list_column_reorder_drag.take() else {
            return Task::none();
        };
        if drag.order_changed {
            self.persist_user_preferences_command()
        } else {
            Task::none()
        }
    }

    pub(super) fn update_list_column_resize_drag(&mut self, position: Point) {
        let Some(drag) = self.list_column_resize_drag else {
            return;
        };

        let resized_width = drag.width_start + position.x - drag.cursor_start_x;
        self.user_config
            .list_view_preferences
            .set_column_width(drag.kind, resized_width);
    }

    pub(super) fn finish_list_column_resize_drag_command(&mut self) -> Task<Message> {
        if self.list_column_resize_drag.take().is_some() {
            self.persist_user_preferences_command()
        } else {
            Task::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use file_core::{SortDirection, SortField};

    use super::*;
    use crate::config;

    #[test]
    fn selecting_list_sort_column_updates_scan_options() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        drop(browser.select_list_sort_column(ListColumnKind::Size));

        assert_eq!(browser.options.sort_field, SortField::Size);
        assert_eq!(browser.options.sort_direction, SortDirection::Ascending);

        drop(browser.select_list_sort_column(ListColumnKind::Size));

        assert_eq!(browser.options.sort_field, SortField::Size);
        assert_eq!(browser.options.sort_direction, SortDirection::Descending);
    }

    #[test]
    fn hiding_current_list_sort_column_updates_scan_options() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        drop(browser.select_list_sort_column(ListColumnKind::Size));
        drop(browser.toggle_list_column_visibility(ListColumnKind::Size));

        assert_eq!(browser.options.sort_field, SortField::Name);
        assert_eq!(browser.options.sort_direction, SortDirection::Ascending);
    }
}
