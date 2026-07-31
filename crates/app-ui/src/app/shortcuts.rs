use iced::keyboard::{self, key};
use iced::{event, Task};

use super::text_input_shortcuts;
use super::FileBrowser;
use crate::model::{Message, PathSuggestionDirection};
use crate::shortcuts::{
    KeyBinding, ShortcutAction, ShortcutBindingId, ShortcutCaptureState, ShortcutConfig,
    ShortcutRoutingContext,
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
        match keyboard_shortcut_route(action, status) {
            KeyboardShortcutRoute::Invoke => self.invoke_shortcut(action),
            KeyboardShortcutRoute::QueryTextInputFocus => {
                text_input_shortcuts::route_ignored_file_content_shortcut(action)
            }
            KeyboardShortcutRoute::CapturedPreview => self.handle_captured_preview_shortcut(),
            KeyboardShortcutRoute::Stop => Task::none(),
        }
    }

    fn handle_path_suggestion_keyboard_key(
        &mut self,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
    ) -> Option<Task<Message>> {
        if !self.address_suggestion_keyboard_is_active() {
            return None;
        }

        match key.as_ref() {
            keyboard::Key::Named(key::Named::ArrowDown) if no_shortcut_modifiers(modifiers) => {
                Some(
                    self.move_path_suggestion_selection_from_keyboard(
                        PathSuggestionDirection::Next,
                    ),
                )
            }
            keyboard::Key::Named(key::Named::ArrowUp) if no_shortcut_modifiers(modifiers) => {
                Some(self.move_path_suggestion_selection_from_keyboard(
                    PathSuggestionDirection::Previous,
                ))
            }
            keyboard::Key::Named(key::Named::Tab) if only_shift_modifier(modifiers) => {
                Some(self.complete_path_suggestion_from_keyboard(PathSuggestionDirection::Previous))
            }
            keyboard::Key::Named(key::Named::Tab) if no_shortcut_modifiers(modifiers) => {
                Some(self.complete_path_suggestion_from_keyboard(PathSuggestionDirection::Next))
            }
            _ => None,
        }
    }

    pub(super) fn file_browser_content_shortcuts_enabled(&self) -> bool {
        self.destructive_action_confirmation.is_none()
            && self.transfer_conflict.is_none()
            && self.context_menu.is_none()
            && self.open_with.is_none()
            && self.archive_creation.is_none()
            && self.archive_extraction.is_none()
            && self.batch_rename.is_none()
            && self.settings_window != Some(self.focused_window)
            && self.properties_window != Some(self.focused_window)
            && self.preview_window != Some(self.focused_window)
            && self.renaming.is_none()
            && self.shortcut_capture.is_none()
            && !self.operation_queue.is_panel_open()
    }

    pub(super) fn invoke_shortcut(&mut self, action: ShortcutAction) -> Task<Message> {
        match action {
            ShortcutAction::OpenSelected => self.activate_selected_path(),
            ShortcutAction::RenameSelected => self.begin_rename_selected(),
            ShortcutAction::FocusPathInput => self.begin_address_editing(self.active_pane_id()),
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
            ShortcutAction::FileProperties => self.open_selected_file_properties(),
            ShortcutAction::Refresh => {
                Task::batch([self.commit_rename_if_active(), self.reload_visible_panes()])
            }
            ShortcutAction::Escape => self.handle_focused_window_escape_pressed(),
            ShortcutAction::Preview => self.request_preview(),
            ShortcutAction::SelectAll => self.select_all_in_file_selection_scope(),
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
        self.persist_user_preferences_command()
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
        self.persist_user_preferences_command()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyboardShortcutRoute {
    Invoke,
    QueryTextInputFocus,
    CapturedPreview,
    Stop,
}

fn keyboard_shortcut_route(action: ShortcutAction, status: event::Status) -> KeyboardShortcutRoute {
    if action.is_preview_toggle() && matches!(status, event::Status::Captured) {
        return KeyboardShortcutRoute::CapturedPreview;
    }

    match (action.routing_context(), status) {
        (ShortcutRoutingContext::Application, _) => KeyboardShortcutRoute::Invoke,
        (ShortcutRoutingContext::FileBrowserContent, event::Status::Captured) => {
            KeyboardShortcutRoute::Stop
        }
        (ShortcutRoutingContext::FileBrowserContent, event::Status::Ignored) => {
            KeyboardShortcutRoute::QueryTextInputFocus
        }
    }
}

fn no_shortcut_modifiers(modifiers: keyboard::Modifiers) -> bool {
    !modifiers.alt() && !modifiers.control() && !modifiers.command() && !modifiers.shift()
}

fn only_shift_modifier(modifiers: keyboard::Modifiers) -> bool {
    !modifiers.alt() && !modifiers.control() && !modifiers.command() && modifiers.shift()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use file_core::{DirectoryEntry, EntryMetadata, FileKind, TrashEntry, TrashRestoreEntry};

    use super::*;
    use crate::config;
    use crate::model::trash_location_path;
    use crate::operation_queue::QueuedFileOperation;

    fn test_entry(path: &Path) -> DirectoryEntry {
        DirectoryEntry::new(
            path.to_path_buf(),
            FileKind::File,
            EntryMetadata {
                len: 0,
                modified: None,
                ..EntryMetadata::default()
            },
            false,
            false,
            false,
        )
    }

    fn browser_with_selected_trash_entry() -> (FileBrowser, TrashRestoreEntry) {
        let trash_path = PathBuf::from("/home/user/.local/share/Trash/files/report.txt");
        let info_path = PathBuf::from("/home/user/.local/share/Trash/info/report.txt.trashinfo");
        let original_path = PathBuf::from("/home/user/report.txt");
        let trash_entry = TrashEntry {
            trash_path: trash_path.clone(),
            info_path: info_path.clone(),
            original_path: original_path.clone(),
            deletion_date: None,
            entry: test_entry(&trash_path),
        };
        let expected_restore_entry = trash_entry.restore_entry();
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        browser.current_dir = trash_location_path();
        browser.is_trash_view = true;
        browser.is_loading = false;
        browser.entries = vec![trash_entry.entry.clone()];
        browser.trash_entries = vec![trash_entry];
        browser.selected = Some(trash_path.clone());
        browser.selected_paths = HashSet::from([trash_path]);
        (browser, expected_restore_entry)
    }

    #[test]
    fn open_selected_shortcut_restores_selected_trash_entry() {
        let (mut browser, expected_restore_entry) = browser_with_selected_trash_entry();

        let command = browser.invoke_shortcut(ShortcutAction::OpenSelected);
        drop(command);

        assert_eq!(browser.operation_queue.tasks().len(), 1);
        assert!(matches!(
            &browser.operation_queue.tasks()[0].operation,
            QueuedFileOperation::Restore { entries } if entries == &vec![expected_restore_entry]
        ));
    }

    #[test]
    fn keyboard_shortcut_route_enforces_context_ownership_matrix() {
        assert_eq!(
            keyboard_shortcut_route(ShortcutAction::SelectAll, event::Status::Captured),
            KeyboardShortcutRoute::Stop
        );
        assert_eq!(
            keyboard_shortcut_route(ShortcutAction::Undo, event::Status::Ignored),
            KeyboardShortcutRoute::QueryTextInputFocus
        );
        assert_eq!(
            keyboard_shortcut_route(ShortcutAction::Escape, event::Status::Captured),
            KeyboardShortcutRoute::Invoke
        );
        assert_eq!(
            keyboard_shortcut_route(ShortcutAction::Preview, event::Status::Captured),
            KeyboardShortcutRoute::CapturedPreview
        );
    }
}
