use iced::keyboard::{self, key};
use iced::{event, Task};

use super::text_input_shortcuts;
use super::FileBrowser;
use crate::model::{Message, OperationQueuePanelMode, PathSuggestionDirection};
use crate::shortcuts::{
    KeyBinding, ShortcutAction, ShortcutBindingId, ShortcutCaptureState, ShortcutConfig,
};

impl FileBrowser {
    pub(crate) fn shortcut_config(&self) -> &ShortcutConfig {
        &self.user_config.shortcuts
    }

    pub(super) fn handle_keyboard_key_pressed(
        &mut self,
        key: keyboard::Key,
        modifiers: keyboard::Modifiers,
        status: event::Status,
    ) -> Task<Message> {
        if self.shortcut_capture.is_some() {
            return match KeyBinding::from_iced_key(&key, modifiers) {
                Some(binding) => self.accept_shortcut_capture(binding),
                None => self.reject_unsupported_shortcut_capture(),
            };
        }

        if let Some(command) = self.handle_path_suggestion_keyboard_key(&key, modifiers) {
            return command;
        }

        let Some(action) = self.user_config.shortcuts.matching_action(&key, modifiers) else {
            return Task::none();
        };
        if action.is_preview_toggle() && matches!(status, event::Status::Captured) {
            return self.handle_captured_preview_shortcut();
        }
        if matches!(status, event::Status::Captured) && !action.bypasses_captured_event() {
            return Task::none();
        }

        self.invoke_shortcut(action)
    }

    fn handle_path_suggestion_keyboard_key(
        &mut self,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
    ) -> Option<Task<Message>> {
        if self.search.is_none() && self.path_suggestions.is_empty() {
            return None;
        }

        match key.as_ref() {
            keyboard::Key::Named(key::Named::ArrowDown) if no_shortcut_modifiers(modifiers) => {
                Some(self.move_search_or_path_suggestion_selection(PathSuggestionDirection::Next))
            }
            keyboard::Key::Named(key::Named::ArrowUp) if no_shortcut_modifiers(modifiers) => Some(
                self.move_search_or_path_suggestion_selection(PathSuggestionDirection::Previous),
            ),
            keyboard::Key::Named(key::Named::Tab) if only_shift_modifier(modifiers) => Some(
                self.complete_search_scope_or_path_suggestion(PathSuggestionDirection::Previous),
            ),
            keyboard::Key::Named(key::Named::Tab) if no_shortcut_modifiers(modifiers) => {
                Some(self.complete_search_scope_or_path_suggestion(PathSuggestionDirection::Next))
            }
            _ => None,
        }
    }

    pub(super) fn file_browser_content_shortcuts_enabled(&self) -> bool {
        self.destructive_action_confirmation.is_none()
            && self.transfer_conflict.is_none()
            && self.context_menu.is_none()
            && self.settings_window != Some(self.focused_window)
            && self.properties_window != Some(self.focused_window)
            && self.renaming.is_none()
            && self.search.is_none()
            && self.shortcut_capture.is_none()
            && !(self.operation_queue.is_panel_open()
                && matches!(
                    self.operation_queue_panel_mode,
                    OperationQueuePanelMode::InteractiveList
                ))
    }

    pub(super) fn invoke_shortcut(&mut self, action: ShortcutAction) -> Task<Message> {
        match action {
            ShortcutAction::OpenSelected => self.activate_selected_path(),
            ShortcutAction::RenameSelected => self.begin_rename_selected(),
            ShortcutAction::FocusPathInput => self.focus_active_path_input(),
            ShortcutAction::NavigateBack => {
                Task::batch([self.commit_rename_if_active(), self.navigate_back()])
            }
            ShortcutAction::NavigateForward => {
                Task::batch([self.commit_rename_if_active(), self.navigate_forward()])
            }
            ShortcutAction::NavigateUp => {
                Task::batch([self.commit_rename_if_active(), self.navigate_up()])
            }
            ShortcutAction::MoveSelection(direction) => self.move_file_selection(direction),
            ShortcutAction::Search if self.is_trash_view => Task::none(),
            ShortcutAction::Search => self.open_search(),
            ShortcutAction::FileProperties => self.open_selected_file_properties(),
            ShortcutAction::Refresh => {
                Task::batch([self.commit_rename_if_active(), self.reload_visible_panes()])
            }
            ShortcutAction::Escape => self.handle_focused_window_escape_pressed(),
            ShortcutAction::Preview => self.request_preview(),
            ShortcutAction::SelectAll => {
                text_input_shortcuts::select_focused_text_or_visible_files_command()
            }
            ShortcutAction::Copy => self.copy_selected(),
            ShortcutAction::Paste => self.paste_pending(),
            ShortcutAction::Cut => self.move_selected(),
            ShortcutAction::Delete => self.trash_selected(),
            ShortcutAction::Undo => self.undo_file_operation(),
            ShortcutAction::Redo => self.redo_file_operation(),
        }
    }

    pub(super) fn start_shortcut_capture(
        &mut self,
        binding_id: ShortcutBindingId,
    ) -> Task<Message> {
        self.shortcut_capture = Some(ShortcutCaptureState::new(binding_id));
        Task::none()
    }

    pub(super) fn cancel_shortcut_capture(&mut self) -> Task<Message> {
        self.shortcut_capture = None;
        Task::none()
    }

    pub(super) fn accept_shortcut_capture(&mut self, binding: KeyBinding) -> Task<Message> {
        let Some(capture) = &self.shortcut_capture else {
            return Task::none();
        };
        let binding_id = capture.binding_id;

        if let Some(conflict) = self
            .user_config
            .shortcuts
            .conflicting_binding(binding_id, &binding)
        {
            self.shortcut_capture = Some(ShortcutCaptureState::conflict(
                binding_id, binding, conflict,
            ));
            return Task::none();
        }

        self.user_config.shortcuts.set_binding(binding_id, binding);
        self.shortcut_capture = None;
        self.persist_user_config_command()
    }

    pub(super) fn reject_unsupported_shortcut_capture(&mut self) -> Task<Message> {
        let Some(capture) = &self.shortcut_capture else {
            return Task::none();
        };
        self.shortcut_capture = Some(ShortcutCaptureState::unsupported(capture.binding_id));
        Task::none()
    }

    pub(super) fn reset_shortcut_binding(
        &mut self,
        binding_id: ShortcutBindingId,
    ) -> Task<Message> {
        let binding = ShortcutConfig::default_binding(binding_id);
        if let Some(conflict) = self
            .user_config
            .shortcuts
            .conflicting_binding(binding_id, &binding)
        {
            self.shortcut_capture = Some(ShortcutCaptureState::conflict(
                binding_id, binding, conflict,
            ));
            return Task::none();
        }

        self.user_config.shortcuts.reset_binding(binding_id);
        self.shortcut_capture = None;
        self.persist_user_config_command()
    }
}

fn no_shortcut_modifiers(modifiers: keyboard::Modifiers) -> bool {
    !modifiers.alt() && !modifiers.control() && !modifiers.command() && !modifiers.shift()
}

fn only_shift_modifier(modifiers: keyboard::Modifiers) -> bool {
    !modifiers.alt() && !modifiers.control() && !modifiers.command() && modifiers.shift()
}
