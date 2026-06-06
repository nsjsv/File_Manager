use iced::keyboard::{self, key, Key};
use iced::{event, mouse, window, Event, Theme};

use crate::model::{Message, PathSuggestionDirection};

pub(super) fn global_event_message(
    event: Event,
    status: event::Status,
    window: window::Id,
) -> Option<Message> {
    if let Event::Window(window_event) = &event {
        return match window_event {
            iced::window::Event::CloseRequested => {
                Some(Message::AuxiliaryWindowCloseRequested(window))
            }
            iced::window::Event::Resized(size) => Some(Message::AuxiliaryWindowResized(
                window,
                size.width,
                size.height,
            )),
            iced::window::Event::Focused => Some(Message::WindowFocused(window)),
            iced::window::Event::Unfocused => Some(Message::WindowUnfocused(window)),
            _ => None,
        };
    }

    if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = &event {
        return Some(Message::KeyboardModifiersChanged(*modifiers));
    }

    if let Some(message) = path_suggestion_keyboard_message(&event) {
        return Some(message);
    }

    if let Some(message) = keyboard_shortcut_message(&event, status) {
        return Some(message);
    }

    if let Event::Mouse(mouse::Event::CursorMoved { position }) = &event {
        return Some(Message::CursorMoved(*position));
    }

    if let Some(message) = pointer_pressed_message(&event, status) {
        return Some(message);
    }

    if matches!(status, event::Status::Captured) {
        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = &event {
            return Some(Message::CapturedWheelScrolled(*delta));
        }
    }

    if matches!(
        &event,
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
    ) && matches!(status, event::Status::Captured)
    {
        return Some(Message::TabDragFinished);
    }

    if captured_preview_shortcut(&event, status) {
        return Some(Message::CapturedPreviewShortcutPressed);
    }

    if matches!(status, event::Status::Captured) {
        return None;
    }

    match event {
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::DragSelectionFinished)
        }
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key::Named::Space),
            ..
        }) => Some(Message::RequestPreview),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)) => Some(Message::Back),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)) => Some(Message::Forward),
        _ => None,
    }
}

fn captured_preview_shortcut(event: &Event, status: event::Status) -> bool {
    matches!(status, event::Status::Captured)
        && matches!(
            event,
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(key::Named::Space),
                ..
            })
        )
}

fn pointer_pressed_message(event: &Event, status: event::Status) -> Option<Message> {
    let Event::Mouse(mouse::Event::ButtonPressed(button)) = event else {
        return None;
    };

    match button {
        mouse::Button::Left | mouse::Button::Right | mouse::Button::Middle => {
            Some(Message::WindowPointerPressed {
                button: *button,
                status,
            })
        }
        _ => None,
    }
}

pub(super) fn system_theme() -> Theme {
    match dark_light::detect() {
        Ok(dark_light::Mode::Dark) => Theme::Dark,
        Ok(dark_light::Mode::Light) | Ok(dark_light::Mode::Unspecified) | Err(_) => Theme::Light,
    }
}

fn keyboard_shortcut_message(event: &Event, status: event::Status) -> Option<Message> {
    let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };

    if captured_widget_owns_text_editing_shortcut(key, *modifiers, status) {
        return None;
    }

    match key.as_ref() {
        Key::Character("a") | Key::Character("A") if primary_modifier(*modifiers) => {
            Some(Message::PrimarySelectAllRequested)
        }
        Key::Character("c") | Key::Character("C") if primary_modifier(*modifiers) => {
            Some(Message::CopySelected)
        }
        Key::Character("f") | Key::Character("F") if primary_modifier(*modifiers) => {
            Some(Message::SearchOpened)
        }
        Key::Character("v") | Key::Character("V") if primary_modifier(*modifiers) => {
            Some(Message::PastePending)
        }
        Key::Character("x") | Key::Character("X") if primary_modifier(*modifiers) => {
            Some(Message::MoveSelected)
        }
        Key::Character("y") | Key::Character("Y") if primary_modifier(*modifiers) => {
            Some(Message::RedoFileOperation)
        }
        Key::Character("z") | Key::Character("Z") if primary_modifier(*modifiers) => {
            Some(Message::UndoFileOperation)
        }
        Key::Named(key::Named::Escape) => Some(Message::FocusedWindowEscapePressed),
        Key::Named(key::Named::Copy) => Some(Message::CopySelected),
        Key::Named(key::Named::Paste) => Some(Message::PastePending),
        Key::Named(key::Named::Cut) => Some(Message::MoveSelected),
        Key::Named(key::Named::Delete) => Some(Message::TrashSelected),
        Key::Named(key::Named::Space) if modifiers.alt() => Some(Message::SearchOpened),
        _ => None,
    }
}

fn captured_widget_owns_text_editing_shortcut(
    key: &Key,
    modifiers: keyboard::Modifiers,
    status: event::Status,
) -> bool {
    matches!(status, event::Status::Captured) && text_editing_shortcut(key, modifiers)
}

fn text_editing_shortcut(key: &Key, modifiers: keyboard::Modifiers) -> bool {
    match key.as_ref() {
        Key::Character("a") | Key::Character("A") if primary_modifier(modifiers) => true,
        Key::Character("c") | Key::Character("C") if primary_modifier(modifiers) => true,
        Key::Character("v") | Key::Character("V") if primary_modifier(modifiers) => true,
        Key::Character("x") | Key::Character("X") if primary_modifier(modifiers) => true,
        Key::Character("y") | Key::Character("Y") if primary_modifier(modifiers) => true,
        Key::Character("z") | Key::Character("Z") if primary_modifier(modifiers) => true,
        Key::Named(key::Named::Copy)
        | Key::Named(key::Named::Paste)
        | Key::Named(key::Named::Cut)
        | Key::Named(key::Named::Delete) => true,
        _ => false,
    }
}

fn primary_modifier(modifiers: keyboard::Modifiers) -> bool {
    modifiers.command() || modifiers.control()
}

fn path_suggestion_keyboard_message(event: &Event) -> Option<Message> {
    let Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(named),
        modifiers,
        ..
    }) = event
    else {
        return None;
    };

    match named {
        key::Named::ArrowDown => Some(Message::PathSuggestionMoved(PathSuggestionDirection::Next)),
        key::Named::ArrowUp => Some(Message::PathSuggestionMoved(
            PathSuggestionDirection::Previous,
        )),
        key::Named::Tab if modifiers.shift() => Some(Message::PathSuggestionCompleted(
            PathSuggestionDirection::Previous,
        )),
        key::Named::Tab => Some(Message::PathSuggestionCompleted(
            PathSuggestionDirection::Next,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route_event(event: Event, status: event::Status) -> Option<Message> {
        global_event_message(event, status, iced::window::Id::unique())
    }

    fn key_pressed(key: Key, modifiers: keyboard::Modifiers) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
            key,
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn captured_clipboard_shortcut_stays_with_focused_widget() {
        let event = key_pressed(Key::Character("v".into()), keyboard::Modifiers::CTRL);

        let message = route_event(event, event::Status::Captured);

        assert!(message.is_none());
    }

    #[test]
    fn ignored_clipboard_shortcut_still_pastes_files() {
        let event = key_pressed(Key::Character("v".into()), keyboard::Modifiers::CTRL);

        let message = route_event(event, event::Status::Ignored);

        assert!(matches!(message, Some(Message::PastePending)));
    }

    #[test]
    fn captured_ctrl_a_stays_with_focused_widget() {
        let event = key_pressed(Key::Character("a".into()), keyboard::Modifiers::CTRL);

        let message = route_event(event, event::Status::Captured);

        assert!(message.is_none());
    }

    #[test]
    fn ignored_ctrl_a_requests_focus_aware_select_all() {
        let event = key_pressed(Key::Character("a".into()), keyboard::Modifiers::CTRL);

        let message = route_event(event, event::Status::Ignored);

        assert!(matches!(message, Some(Message::PrimarySelectAllRequested)));
    }

    #[test]
    fn captured_left_release_finishes_tab_drag() {
        let event = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));

        let message = route_event(event, event::Status::Captured);

        assert!(matches!(message, Some(Message::TabDragFinished)));
    }

    #[test]
    fn captured_cursor_move_updates_cursor_position() {
        let position = iced::Point::new(12.0, 24.0);
        let event = Event::Mouse(mouse::Event::CursorMoved { position });

        let message = route_event(event, event::Status::Captured);

        let Some(Message::CursorMoved(received)) = message else {
            panic!("expected cursor movement message");
        };
        assert_eq!(received.x, position.x);
        assert_eq!(received.y, position.y);
    }

    #[test]
    fn captured_wheel_scroll_reports_scrollbar_activity() {
        let delta = mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 };
        let event = Event::Mouse(mouse::Event::WheelScrolled { delta });

        let message = route_event(event, event::Status::Captured);

        assert!(matches!(
            message,
            Some(Message::CapturedWheelScrolled(received)) if received == delta
        ));
    }

    #[test]
    fn ctrl_f_opens_search_even_when_widget_captured_keyboard() {
        let event = key_pressed(Key::Character("f".into()), keyboard::Modifiers::CTRL);

        let message = route_event(event, event::Status::Captured);

        assert!(matches!(message, Some(Message::SearchOpened)));
    }

    #[test]
    fn captured_ctrl_z_stays_with_focused_widget() {
        let event = key_pressed(Key::Character("z".into()), keyboard::Modifiers::CTRL);

        let message = route_event(event, event::Status::Captured);

        assert!(message.is_none());
    }

    #[test]
    fn ignored_ctrl_z_requests_file_operation_undo() {
        let event = key_pressed(Key::Character("z".into()), keyboard::Modifiers::CTRL);

        let message = route_event(event, event::Status::Ignored);

        assert!(matches!(message, Some(Message::UndoFileOperation)));
    }

    #[test]
    fn captured_ctrl_y_stays_with_focused_widget() {
        let event = key_pressed(Key::Character("y".into()), keyboard::Modifiers::CTRL);

        let message = route_event(event, event::Status::Captured);

        assert!(message.is_none());
    }

    #[test]
    fn ignored_ctrl_y_requests_file_operation_redo() {
        let event = key_pressed(Key::Character("y".into()), keyboard::Modifiers::CTRL);

        let message = route_event(event, event::Status::Ignored);

        assert!(matches!(message, Some(Message::RedoFileOperation)));
    }

    #[test]
    fn captured_delete_stays_with_focused_widget() {
        let event = key_pressed(
            Key::Named(key::Named::Delete),
            keyboard::Modifiers::default(),
        );

        let message = route_event(event, event::Status::Captured);

        assert!(message.is_none());
    }

    #[test]
    fn ignored_delete_trashes_selected_files() {
        let event = key_pressed(
            Key::Named(key::Named::Delete),
            keyboard::Modifiers::default(),
        );

        let message = route_event(event, event::Status::Ignored);

        assert!(matches!(message, Some(Message::TrashSelected)));
    }

    #[test]
    fn alt_space_opens_search_instead_of_preview() {
        let event = key_pressed(Key::Named(key::Named::Space), keyboard::Modifiers::ALT);

        let message = route_event(event, event::Status::Ignored);

        assert!(matches!(message, Some(Message::SearchOpened)));
    }

    #[test]
    fn captured_space_reports_preview_shortcut() {
        let event = key_pressed(
            Key::Named(key::Named::Space),
            keyboard::Modifiers::default(),
        );

        let message = route_event(event, event::Status::Captured);

        assert!(matches!(
            message,
            Some(Message::CapturedPreviewShortcutPressed)
        ));
    }

    #[test]
    fn escape_reports_focused_window_shortcut_even_when_widget_captured_keyboard() {
        let event = key_pressed(
            Key::Named(key::Named::Escape),
            keyboard::Modifiers::default(),
        );

        let message = route_event(event, event::Status::Captured);

        assert!(matches!(message, Some(Message::FocusedWindowEscapePressed)));
    }

    #[test]
    fn focused_window_event_updates_window_focus_state() {
        let window = iced::window::Id::unique();
        let event = Event::Window(iced::window::Event::Focused);

        let message = global_event_message(event, event::Status::Ignored, window);

        assert!(matches!(
            message,
            Some(Message::WindowFocused(focused_window)) if focused_window == window
        ));
    }

    #[test]
    fn unfocused_window_event_updates_window_focus_state() {
        let window = iced::window::Id::unique();
        let event = Event::Window(iced::window::Event::Unfocused);

        let message = global_event_message(event, event::Status::Ignored, window);

        assert!(matches!(
            message,
            Some(Message::WindowUnfocused(unfocused_window)) if unfocused_window == window
        ));
    }

    #[test]
    fn captured_left_press_reports_pointer_press() {
        let event = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));

        let message = route_event(event, event::Status::Captured);

        assert!(matches!(
            message,
            Some(Message::WindowPointerPressed {
                button: mouse::Button::Left,
                status: event::Status::Captured,
            })
        ));
    }
}
